use crate::bilibili::{
    ApiError, InputTarget, PlayData, QrLogin, QrState, Quality, SeasonPage, VideoInfo,
    parse_bangumi_response, parse_input, parse_media_season_id, parse_play_response,
    parse_qr_generate, parse_qr_poll, parse_season_response, parse_view_response,
};
use crate::download::DownloadItem;
use crate::http::{HttpClient, HttpError};
use std::path::Path;
use std::sync::atomic::AtomicBool;

const API_ROOT: &str = "https://api.bilibili.com";
const VIDEO_REFERER: &str = "https://www.bilibili.com/";

#[derive(Default)]
pub struct BiliClient {
    http: HttpClient,
    authenticated: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedContent {
    pub title: String,
    pub items: Vec<DownloadItem>,
    pub qualities: Vec<Quality>,
    pub access_notice: Option<String>,
    pub focus_page: u32,
    pub is_collection: bool,
}

impl BiliClient {
    pub fn is_logged_in(&self) -> bool {
        self.authenticated
    }

    pub fn restore_session(&mut self) -> Result<bool, String> {
        let Some(cookie) = crate::session::load()? else {
            return Ok(false);
        };
        self.http.set_cookie_header(cookie)?;
        match self.verify_login() {
            Ok(()) => Ok(true),
            Err(error) => {
                self.authenticated = false;
                self.http.set_cookie_header(String::new())?;
                if error == ApiError::LoginRequired {
                    crate::session::forget()?;
                    Ok(false)
                } else {
                    Err(format!("暂时无法验证已保存的登录：{error}"))
                }
            }
        }
    }

    pub fn remember_session(&self) -> Result<(), String> {
        if !self.authenticated {
            return Err("登录完成后才能保存登录记录".to_string());
        }
        crate::session::save(self.http.cookie_header())
    }

    pub fn logout(&mut self) -> Result<(), String> {
        crate::session::forget()?;
        self.http.set_cookie_header(String::new())?;
        self.authenticated = false;
        Ok(())
    }

    pub fn video_info(&mut self, bvid: &str) -> Result<VideoInfo, ApiError> {
        let body = self.get_json(&format!("{API_ROOT}/x/web-interface/view?bvid={bvid}"))?;
        parse_view_response(&body)
    }

    pub fn season_page(
        &mut self,
        mid: u64,
        season_id: u64,
        page: u32,
    ) -> Result<SeasonPage, ApiError> {
        let body = self.get_json(&format!(
            "{API_ROOT}/x/polymer/web-space/seasons_archives_list?mid={mid}&season_id={season_id}&page_num={page}&page_size=20"
        ))?;
        parse_season_response(&body, season_id)
    }

    pub fn play(
        &mut self,
        bvid: &str,
        cid: u64,
        quality: u32,
        ep_id: Option<u64>,
    ) -> Result<PlayData, ApiError> {
        let url = match ep_id {
            Some(id) => format!(
                "{API_ROOT}/pgc/player/web/v2/playurl?ep_id={id}&cid={cid}&qn={quality}&fnval=4048&fnver=0&fourk=1"
            ),
            None => format!(
                "{API_ROOT}/x/player/playurl?bvid={bvid}&cid={cid}&qn={quality}&fnval=4048&fnver=0&fourk=1"
            ),
        };
        let body = self.get_json(&url)?;
        let result = parse_play_response(&body);
        if matches!(result, Err(ApiError::LoginRequired)) {
            self.authenticated = false;
            self.http
                .set_cookie_header(String::new())
                .map_err(ApiError::Message)?;
            crate::session::forget().map_err(ApiError::Message)?;
        }
        match result {
            Err(ApiError::PermissionDenied(message))
                if !self.authenticated
                    && ["大会员", "充电", "登录", "账号"]
                        .iter()
                        .any(|word| message.contains(word)) =>
            {
                Err(ApiError::LoginRequired)
            }
            result => result,
        }
    }

    pub fn qr_generate(&mut self) -> Result<QrLogin, ApiError> {
        let body =
            self.get_json("https://passport.bilibili.com/x/passport-login/web/qrcode/generate")?;
        parse_qr_generate(&body)
    }

    pub fn qr_poll(&mut self, key: &str) -> Result<QrState, ApiError> {
        let body = self.get_json(&format!(
            "https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key={key}"
        ))?;
        let state = parse_qr_poll(&body)?;
        if state == QrState::Confirmed {
            self.verify_login()?;
        }
        Ok(state)
    }

    pub fn download_to(
        &mut self,
        url: &str,
        path: &Path,
        cancel: &AtomicBool,
        progress: impl FnMut(u64, Option<u64>),
    ) -> Result<(), HttpError> {
        let headers = [
            ("Referer", VIDEO_REFERER),
            ("Origin", "https://www.bilibili.com"),
            ("Accept", "*/*"),
        ];
        self.http.download_to(url, &headers, path, cancel, progress)
    }

    pub fn resolve(&mut self, input: &str) -> Result<ResolvedContent, ApiError> {
        let parsed = parse_input(input).map_err(ApiError::Message)?;
        let mut items = Vec::new();
        let (title, is_collection) = match parsed.target {
            InputTarget::Video(bvid) => {
                let info = self.video_info(&bvid)?;
                let title = info.title.clone();
                items = expand_video(info, 1);
                (title, false)
            }
            InputTarget::Season { mid, season_id } => {
                let mut page_number = 1;
                let mut archive_count = 0_u32;
                loop {
                    let page = self.season_page(mid, season_id, page_number)?;
                    let total = page.total;
                    let received = page.archives.len();
                    archive_count += received as u32;
                    for archive in page.archives {
                        let info = self.video_info(&archive.bvid)?;
                        let sequence = items.len() as u32 + 1;
                        items.extend(expand_video(info, sequence));
                    }
                    if received == 0
                        || archive_count >= total
                        || received < 20
                        || page_number >= 100
                    {
                        break;
                    }
                    page_number += 1;
                }
                (format!("合集 {season_id}"), true)
            }
            target @ (InputTarget::BangumiEpisode(_)
            | InputTarget::BangumiSeason(_)
            | InputTarget::BangumiMedia(_)) => {
                let query = match target {
                    InputTarget::BangumiEpisode(id) => format!("ep_id={id}"),
                    InputTarget::BangumiSeason(id) => format!("season_id={id}"),
                    InputTarget::BangumiMedia(id) => {
                        let body =
                            self.get_json(&format!("{API_ROOT}/pgc/review/user?media_id={id}"))?;
                        format!("season_id={}", parse_media_season_id(&body)?)
                    }
                    _ => unreachable!(),
                };
                let body = self.get_json(&format!("{API_ROOT}/pgc/view/web/season?{query}"))?;
                let info = parse_bangumi_response(&body)?;
                let single_episode = matches!(target, InputTarget::BangumiEpisode(_));
                for episode in info.episodes {
                    if let InputTarget::BangumiEpisode(id) = target
                        && id != episode.id
                    {
                        continue;
                    }
                    let sequence = items.len() as u32 + 1;
                    items.push(DownloadItem {
                        bvid: episode.bvid,
                        ep_id: Some(episode.id),
                        title: info.title.clone(),
                        part: episode.title,
                        page: sequence,
                        cid: episode.cid,
                        duration: episode.duration,
                        sequence,
                    });
                }
                (info.title, !single_episode)
            }
        };
        if items.is_empty() {
            return Err(ApiError::Message(
                "没有解析到可下载的分 P 或剧集，内容可能尚未上线".to_string(),
            ));
        }
        if !is_collection && !items.iter().any(|item| item.page == parsed.page) {
            return Err(ApiError::Message(format!(
                "链接指定的分 P {} 不存在",
                parsed.page
            )));
        }
        let first = &items[0];
        let (qualities, access_notice) = match self.play(&first.bvid, first.cid, 80, first.ep_id) {
            Ok(play) if !play.qualities.is_empty() => (play.qualities, None),
            Ok(_) => (default_qualities(), None),
            Err(error) => (default_qualities(), Some(error.to_string())),
        };
        Ok(ResolvedContent {
            title,
            items,
            qualities,
            access_notice,
            focus_page: parsed.page,
            is_collection,
        })
    }

    fn get_json(&mut self, url: &str) -> Result<String, ApiError> {
        let response = self
            .http
            .get(
                url,
                &[("Referer", VIDEO_REFERER), ("Accept", "application/json")],
            )
            .map_err(http_api_error)?;
        if !(200..300).contains(&response.status) {
            return Err(ApiError::Message(format!(
                "HTTP {}: {}",
                response.status,
                response.text().chars().take(200).collect::<String>()
            )));
        }
        Ok(response.text())
    }

    fn verify_login(&mut self) -> Result<(), ApiError> {
        let body = self.get_json(&format!("{API_ROOT}/x/web-interface/nav"))?;
        let value: serde_json::Value = serde_json::from_str(&body)
            .map_err(|error| ApiError::Message(format!("登录校验响应无效: {error}")))?;
        let code = value
            .get("code")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| ApiError::Message("登录校验响应缺少状态码".to_string()))?;
        if code == -101 {
            self.authenticated = false;
            return Err(ApiError::LoginRequired);
        }
        if code != 0 {
            return Err(ApiError::Message(format!(
                "登录校验暂时失败（接口状态 {code}）"
            )));
        }
        let logged_in = value
            .pointer("/data/isLogin")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| ApiError::Message("登录校验响应缺少登录状态".to_string()))?;
        self.authenticated = logged_in;
        if logged_in {
            self.authenticated = true;
            Ok(())
        } else {
            Err(ApiError::LoginRequired)
        }
    }
}

fn http_api_error(error: HttpError) -> ApiError {
    ApiError::Message(error.to_string())
}

pub fn expand_video(info: VideoInfo, first_sequence: u32) -> Vec<DownloadItem> {
    info.pages
        .into_iter()
        .enumerate()
        .map(|(index, page)| DownloadItem {
            bvid: info.bvid.clone(),
            ep_id: None,
            title: info.title.clone(),
            part: page.part,
            page: page.page,
            cid: page.cid,
            duration: page.duration,
            sequence: first_sequence + index as u32,
        })
        .collect()
}

fn default_qualities() -> Vec<Quality> {
    [
        ("高清 1080P", 80),
        ("高清 720P", 64),
        ("清晰 480P", 32),
        ("流畅 360P", 16),
    ]
    .into_iter()
    .map(|(label, id)| Quality {
        id,
        label: label.to_string(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::BiliClient;
    use crate::bilibili::{VideoInfo, VideoPage};

    #[test]
    #[ignore = "requires live Bilibili PGC network access"]
    fn resolves_bangumi_episode_season_and_media_with_pgc_playback() {
        let mut client = BiliClient::default();
        let episode = client
            .resolve("https://www.bilibili.com/bangumi/play/ep232465")
            .expect("episode");
        assert_eq!(episode.title, "工作细胞");
        assert_eq!(episode.items.len(), 1);
        assert_eq!(episode.items[0].ep_id, Some(232465));
        assert!(!episode.is_collection);
        let season = client.resolve("ss24588").expect("season");
        let media = client.resolve("md102392").expect("media");
        assert!(season.is_collection);
        assert!(season.items.len() >= 14);
        assert_eq!(season.items, media.items);
        let movie = client.resolve("ss12548").expect("film");
        assert_eq!(movie.title, "让子弹飞");
        assert_eq!(movie.items[0].ep_id, Some(199612));
        assert!(movie.items[0].duration > 7000);
        let item = &episode.items[0];
        let play = client
            .play(&item.bvid, item.cid, 32, item.ep_id)
            .expect("free episode playback");
        assert!(!play.videos.is_empty() || !play.direct_urls.is_empty());
        assert!(play.media_duration_ms / 1000 + 2 >= item.duration);
        assert_eq!(
            client.play("BV14s411J7py", 46285306, 32, Some(232466)),
            Err(crate::bilibili::ApiError::LoginRequired)
        );
    }

    #[test]
    #[ignore = "requires live Bilibili network access"]
    fn reads_real_public_video_metadata() {
        let mut client = BiliClient::default();
        let info = client.video_info("BV1K2C2BSEZB").expect("public metadata");
        assert!(info.title.contains("FlowMatching"));
        assert_eq!(info.bvid, "BV1K2C2BSEZB");
        assert!(!info.pages.is_empty());
    }

    #[test]
    #[ignore = "requires live Bilibili network access"]
    fn unauthenticated_play_is_shorter_than_full_video() {
        let mut client = BiliClient::default();
        let info = client.video_info("BV1K2C2BSEZB").expect("public metadata");
        let page = info.pages.first().expect("first page");
        let play = client
            .play(&info.bvid, page.cid, 80, None)
            .expect("play data");
        assert!(play.duration_ms / 1000 < page.duration);
    }

    #[test]
    #[ignore = "requires live Bilibili network access"]
    fn resolves_real_public_season_collection() {
        let mut client = BiliClient::default();
        let content = client
            .resolve(
                "https://space.bilibili.com/18235884/lists?sid=6490480&spm_id_from=333.788.0.0",
            )
            .expect("public season");
        assert!(content.is_collection);
        assert!(content.items.len() >= 10);
        assert!(!content.qualities.is_empty());
    }

    #[test]
    #[ignore = "requires live Bilibili network access"]
    fn discards_expired_saved_session_in_isolated_profile() {
        if std::env::var_os("BILIPICK_EXPIRED_SESSION_TEST").is_some() {
            let mut client = BiliClient::default();
            assert!(!client.restore_session().unwrap());
            assert!(client.remember_session().is_err());
            crate::session::save("SESSDATA=bilipick-expired-test").unwrap();
            assert!(!client.restore_session().unwrap());
            assert!(!client.is_logged_in());
            assert!(!crate::session::session_path().unwrap().exists());
            client.logout().unwrap();
            return;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("bilipick-expired-session-{stamp}"));
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "service::tests::discards_expired_saved_session_in_isolated_profile",
                "--ignored",
                "--nocapture",
            ])
            .env("LOCALAPPDATA", &root)
            .env("BILIPICK_EXPIRED_SESSION_TEST", "1")
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::remove_dir(root.join("BiliPick")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn expands_each_video_page_into_the_queue() {
        let info = VideoInfo {
            bvid: "BV1K2C2BSEZB".to_string(),
            title: "Series".to_string(),
            duration: 30,
            pages: vec![
                VideoPage {
                    page: 1,
                    part: "First".to_string(),
                    duration: 10,
                    cid: 1,
                },
                VideoPage {
                    page: 2,
                    part: "Second".to_string(),
                    duration: 20,
                    cid: 2,
                },
            ],
        };
        let items = super::expand_video(info, 4);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].sequence, 4);
        assert_eq!(items[1].sequence, 5);
        assert_eq!(items[1].part, "Second");
    }
}
