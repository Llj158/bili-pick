use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputTarget {
    Video(String),
    Season { mid: u64, season_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quality {
    pub id: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaStream {
    pub id: u32,
    pub base_url: String,
    pub codec: String,
    pub bandwidth: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoInfo {
    pub bvid: String,
    pub title: String,
    pub duration: u64,
    pub pages: Vec<VideoPage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPage {
    pub page: u32,
    pub part: String,
    pub duration: u64,
    pub cid: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonArchive {
    pub bvid: String,
    pub title: String,
    pub duration: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonPage {
    pub total: u32,
    pub archives: Vec<SeasonArchive>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayData {
    pub qualities: Vec<Quality>,
    pub videos: Vec<MediaStream>,
    pub audios: Vec<MediaStream>,
    pub direct_urls: Vec<String>,
    pub direct_size: Option<u64>,
    pub duration_ms: u64,
    pub media_duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    LoginRequired,
    PermissionDenied(String),
    Message(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoginRequired => formatter.write_str("该内容需要登录"),
            Self::PermissionDenied(message) | Self::Message(message) => {
                formatter.write_str(message)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrLogin {
    pub url: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrState {
    Waiting,
    Scanned,
    Expired,
    Confirmed,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiliInput {
    pub target: InputTarget,
    pub page: u32,
}

pub fn parse_input(input: &str) -> Result<BiliInput, String> {
    let value = input.trim();
    if let Some(bvid) = find_bvid(value) {
        return Ok(BiliInput {
            target: InputTarget::Video(bvid.to_string()),
            page: parse_page(value)?,
        });
    }
    if value.starts_with("https://space.bilibili.com/") {
        let value = value.split('#').next().unwrap_or(value);
        let (path, query) = value.split_once('?').unwrap_or((value, ""));
        let path = path.trim_end_matches('/');
        let parts: Vec<&str> = path.split('/').collect();
        if (parts.len() == 5 || parts.len() == 6) && parts[4] == "lists" {
            let mid = parts[3]
                .parse::<u64>()
                .map_err(|_| "合集链接中的用户 ID 无效".to_string())?;
            let query_id = query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == "sid").then_some(value)
            });
            let raw_id = parts
                .get(5)
                .copied()
                .or(query_id)
                .ok_or_else(|| "合集链接缺少 sid 参数".to_string())?;
            let season_id = raw_id
                .parse::<u64>()
                .map_err(|_| "合集链接中的 ID 无效".to_string())?;
            if mid == 0 || season_id == 0 {
                return Err("合集链接中的 ID 必须大于 0".to_string());
            }
            return Ok(BiliInput {
                target: InputTarget::Season { mid, season_id },
                page: 1,
            });
        }
    }
    Err("请输入 BV 号、视频链接或合集链接".to_string())
}

fn find_bvid(value: &str) -> Option<&str> {
    value.match_indices("BV").find_map(|(index, _)| {
        let candidate = value.get(index..index + 12)?;
        candidate[2..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
            .then_some(candidate)
    })
}

fn parse_page(value: &str) -> Result<u32, String> {
    let query = value.split_once('?').map_or("", |(_, query)| query);
    for pair in query.split('&') {
        if let Some((key, raw)) = pair.split_once('=')
            && key == "p"
        {
            let page = raw.parse().map_err(|_| "分 P 参数无效".to_string())?;
            if page == 0 {
                return Err("分 P 参数必须大于 0".to_string());
            }
            return Ok(page);
        }
    }
    Ok(1)
}

const MIXIN_SALT: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

pub fn mixin_key(img_key: &str, sub_key: &str) -> String {
    let raw: Vec<char> = format!("{img_key}{sub_key}").chars().collect();
    MIXIN_SALT
        .iter()
        .take(32)
        .map(|&index| raw[index])
        .collect()
}

pub fn wbi_signed_query(params: &[(&str, &str)], mixin_key_value: &str, timestamp: u64) -> String {
    let mut pairs: Vec<(String, String)> = params
        .iter()
        .map(|(key, value)| {
            (
                percent_encode(&filter_wbi_value(key)),
                percent_encode(&filter_wbi_value(value)),
            )
        })
        .collect();
    pairs.push(("wts".to_string(), timestamp.to_string()));
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let query = pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let signature = md5_hex(&format!("{query}{mixin_key_value}"));
    format!("{query}&w_rid={signature}")
}

fn filter_wbi_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn md5_hex(input: &str) -> String {
    let mut bytes = input.as_bytes().to_vec();
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    bytes.push(0x80);
    while bytes.len() % 64 != 56 {
        bytes.push(0);
    }
    bytes.extend_from_slice(&bit_len.to_le_bytes());

    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const TABLE: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let mut state = [0x67452301_u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in bytes.as_chunks::<64>().0 {
        let mut words = [0_u32; 16];
        for (word, slot) in words.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*slot);
        }

        let [mut a, mut b, mut c, mut d] = state;
        for index in 0..64 {
            let (mixed, word_index) = match index {
                0..=15 => ((b & c) | (!b & d), index),
                16..=31 => ((d & b) | (!d & c), (5 * index + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * index + 5) % 16),
                _ => (c ^ (b | !d), (7 * index) % 16),
            };
            let next_b = b.wrapping_add(
                a.wrapping_add(mixed)
                    .wrapping_add(TABLE[index])
                    .wrapping_add(words[word_index])
                    .rotate_left(SHIFTS[index]),
            );
            a = d;
            d = c;
            c = b;
            b = next_b;
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d]) {
            *slot = slot.wrapping_add(value);
        }
    }

    state
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn quality_options(ids: &[u32], descriptions: &[&str]) -> Vec<Quality> {
    ids.iter()
        .zip(descriptions)
        .map(|(&id, label)| Quality {
            id,
            label: (*label).to_string(),
        })
        .collect()
}

pub fn select_video_stream(streams: &[MediaStream], requested_id: u32) -> Option<&MediaStream> {
    let within_request = |stream: &&MediaStream| stream.id <= requested_id;
    streams
        .iter()
        .filter(within_request)
        .filter(|stream| stream.codec.starts_with("avc1"))
        .max_by_key(|stream| (stream.id, stream.bandwidth))
        .or_else(|| {
            streams
                .iter()
                .filter(within_request)
                .max_by_key(|stream| (stream.id, stream.bandwidth))
        })
}

pub fn select_audio_stream(streams: &[MediaStream]) -> Option<&MediaStream> {
    streams
        .iter()
        .filter(|stream| stream.codec.starts_with("mp4a"))
        .max_by_key(|stream| stream.bandwidth)
        .or_else(|| streams.iter().max_by_key(|stream| stream.bandwidth))
}

pub fn parse_view_response(input: &str) -> Result<VideoInfo, ApiError> {
    let root = parse_api_root(input)?;
    let data = root
        .get("data")
        .ok_or_else(|| ApiError::Message("视频信息响应缺少 data".to_string()))?;
    let pages = data
        .get("pages")
        .and_then(serde_json::Value::as_array)
        .map(|pages| pages.iter().filter_map(parse_video_page).collect())
        .unwrap_or_default();
    Ok(VideoInfo {
        bvid: string_field(data, "bvid")?,
        title: string_field(data, "title")?,
        duration: u64_field(data, "duration").unwrap_or_default(),
        pages,
    })
}

pub fn parse_season_response(input: &str, expected_season_id: u64) -> Result<SeasonPage, ApiError> {
    let root = parse_api_root(input)?;
    let data = root
        .get("data")
        .ok_or_else(|| ApiError::Message("合集响应缺少 data".to_string()))?;
    let meta = data
        .get("meta")
        .ok_or_else(|| ApiError::Message("合集响应缺少 meta".to_string()))?;
    let season_id = meta
        .get("season_id")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ApiError::Message("合集响应缺少 season_id".to_string()))?;
    if season_id != expected_season_id {
        return Err(ApiError::Message("接口返回了其他合集".to_string()));
    }
    let total = data
        .get("page")
        .and_then(|page| page.get("total"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default() as u32;
    let archives = data
        .get("archives")
        .and_then(serde_json::Value::as_array)
        .map(|archives| archives.iter().filter_map(parse_archive).collect())
        .unwrap_or_default();
    Ok(SeasonPage { total, archives })
}

pub fn parse_play_response(input: &str) -> Result<PlayData, ApiError> {
    let root = parse_api_root(input)?;
    let data = root
        .get("data")
        .ok_or_else(|| ApiError::Message("播放信息响应缺少 data".to_string()))?;
    let ids: Vec<u32> = data
        .get("accept_quality")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_u64)
                .map(|value| value as u32)
                .collect()
        })
        .unwrap_or_default();
    let descriptions: Vec<String> = data
        .get("accept_description")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let descriptions: Vec<&str> = descriptions.iter().map(String::as_str).collect();
    let videos = parse_streams(data.pointer("/dash/video"));
    let audios = parse_streams(data.pointer("/dash/audio"));
    let direct_urls = data
        .get("durl")
        .and_then(serde_json::Value::as_array)
        .map(|segments| {
            segments
                .iter()
                .filter_map(|segment| segment.get("url"))
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let direct_size = data
        .get("durl")
        .and_then(serde_json::Value::as_array)
        .and_then(|segments| {
            segments.iter().try_fold(0_u64, |sum, segment| {
                segment
                    .get("size")
                    .and_then(serde_json::Value::as_u64)
                    .map(|size| sum + size)
            })
        });
    let direct_duration_ms = data
        .get("durl")
        .and_then(serde_json::Value::as_array)
        .map(|segments| {
            segments
                .iter()
                .filter_map(|segment| segment.get("length").and_then(serde_json::Value::as_u64))
                .sum()
        })
        .unwrap_or_default();
    let duration_ms = data
        .get("timelength")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    Ok(PlayData {
        qualities: quality_options(&ids, &descriptions),
        videos,
        audios,
        direct_urls,
        direct_size,
        duration_ms,
        media_duration_ms: if direct_duration_ms > 0 {
            direct_duration_ms
        } else {
            duration_ms
        },
    })
}

pub fn is_official_qr_url(url: &str) -> bool {
    let Some(remainder) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = remainder.split('/').next().unwrap_or_default();
    matches!(authority, "account.bilibili.com" | "passport.bilibili.com")
}

pub fn parse_qr_generate(input: &str) -> Result<QrLogin, ApiError> {
    let root = parse_api_root(input)?;
    let data = root
        .get("data")
        .ok_or_else(|| ApiError::Message("二维码响应缺少 data".to_string()))?;
    let url = string_field(data, "url")?;
    if !is_official_qr_url(&url) {
        return Err(ApiError::PermissionDenied(
            "二维码地址不是受信任的官方 HTTPS 域名".to_string(),
        ));
    }
    Ok(QrLogin {
        url,
        key: string_field(data, "qrcode_key")?,
    })
}

pub fn parse_qr_poll(input: &str) -> Result<QrState, ApiError> {
    let root = parse_api_root(input)?;
    let data = root
        .get("data")
        .ok_or_else(|| ApiError::Message("扫码状态响应缺少 data".to_string()))?;
    let code = data
        .get("code")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(-1);
    let message = data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    Ok(match code {
        0 => QrState::Confirmed,
        86090 => QrState::Scanned,
        86038 => QrState::Expired,
        86101 => QrState::Waiting,
        _ => QrState::Failed(format!("扫码状态 {code}: {message}")),
    })
}

fn parse_api_root(input: &str) -> Result<serde_json::Value, ApiError> {
    let root: serde_json::Value = serde_json::from_str(input)
        .map_err(|error| ApiError::Message(format!("接口返回的 JSON 无法解析: {error}")))?;
    let code = root
        .get("code")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    if code == 0 {
        return Ok(root);
    }
    let message = root
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("未知接口错误")
        .to_string();
    if code == -101 || message.contains("登录") {
        Err(ApiError::LoginRequired)
    } else if ["大会员", "充电", "权限", "专享", "付费", "购买"]
        .iter()
        .any(|word| message.contains(word))
    {
        Err(ApiError::PermissionDenied(message))
    } else {
        Err(ApiError::Message(format!("接口错误 {code}: {message}")))
    }
}

fn parse_video_page(value: &serde_json::Value) -> Option<VideoPage> {
    Some(VideoPage {
        page: value.get("page")?.as_u64()? as u32,
        part: value.get("part")?.as_str()?.to_string(),
        duration: value.get("duration")?.as_u64()?,
        cid: value.get("cid")?.as_u64()?,
    })
}

fn parse_archive(value: &serde_json::Value) -> Option<SeasonArchive> {
    Some(SeasonArchive {
        bvid: value.get("bvid")?.as_str()?.to_string(),
        title: value.get("title")?.as_str()?.to_string(),
        duration: value.get("duration")?.as_u64()?,
    })
}

fn parse_streams(value: Option<&serde_json::Value>) -> Vec<MediaStream> {
    value
        .and_then(serde_json::Value::as_array)
        .map(|streams| {
            streams
                .iter()
                .filter_map(|stream| {
                    Some(MediaStream {
                        id: stream.get("id")?.as_u64()? as u32,
                        base_url: stream
                            .get("baseUrl")
                            .or_else(|| stream.get("base_url"))?
                            .as_str()?
                            .to_string(),
                        codec: stream.get("codecs")?.as_str()?.to_string(),
                        bandwidth: stream
                            .get("bandwidth")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn string_field(value: &serde_json::Value, key: &str) -> Result<String, ApiError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ApiError::Message(format!("响应缺少字段 {key}")))
}

fn u64_field(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::{
        ApiError, BiliInput, InputTarget, MediaStream, PlayData, QrLogin, QrState, Quality,
        SeasonArchive, SeasonPage, VideoInfo, VideoPage, is_official_qr_url, md5_hex, mixin_key,
        parse_input, parse_play_response, parse_qr_generate, parse_qr_poll, parse_season_response,
        parse_view_response, quality_options, select_audio_stream, select_video_stream,
        wbi_signed_query,
    };

    #[test]
    fn parses_bare_bv_as_first_video_page() {
        assert_eq!(
            parse_input("BV1K2C2BSEZB"),
            Ok(BiliInput {
                target: InputTarget::Video("BV1K2C2BSEZB".to_string()),
                page: 1,
            })
        );
    }

    #[test]
    fn parses_video_url_and_preserves_page() {
        assert_eq!(
            parse_input("https://www.bilibili.com/video/BV1K2C2BSEZB/?p=3&spm_id_from=333.999"),
            Ok(BiliInput {
                target: InputTarget::Video("BV1K2C2BSEZB".to_string()),
                page: 3,
            })
        );
    }

    #[test]
    fn parses_season_url() {
        assert_eq!(
            parse_input("https://space.bilibili.com/18235884/lists/6490480?type=season"),
            Ok(BiliInput {
                target: InputTarget::Season {
                    mid: 18235884,
                    season_id: 6490480,
                },
                page: 1,
            })
        );
    }

    #[test]
    fn parses_query_season_links_and_ignores_share_parameters() {
        for url in [
            "https://space.bilibili.com/18235884/lists?sid=6490480&spm_id_from=333.788.0.0",
            "https://space.bilibili.com/18235884/lists/?spm_id_from=333.788&sid=6490480#video",
        ] {
            assert_eq!(
                parse_input(url).unwrap().target,
                InputTarget::Season {
                    mid: 18235884,
                    season_id: 6490480
                }
            );
        }
    }

    #[test]
    fn rejects_missing_invalid_or_zero_season_ids() {
        for suffix in ["", "?sid=", "?sid=oops", "?sid=0", "/0?type=season"] {
            assert!(
                parse_input(&format!(
                    "https://space.bilibili.com/18235884/lists{suffix}"
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn md5_matches_known_vectors() {
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn builds_mixin_key_with_fixed_permutation() {
        assert_eq!(
            mixin_key(
                "7cd084941338484aae1ad9425b84077c",
                "4932caff0ff746eab6f01bf08b70ac45",
            ),
            "ea1db124af3c7062474693fa704f4ff8"
        );
    }

    #[test]
    fn signs_sorted_wbi_query() {
        assert_eq!(
            wbi_signed_query(
                &[("foo", "114"), ("bar", "514"), ("zab", "1919810")],
                "ea1db124af3c7062474693fa704f4ff8",
                1_702_204_169,
            ),
            "bar=514&foo=114&wts=1702204169&zab=1919810&w_rid=8f6f2b5b3d485fe1886cec6a0be8c5d4"
        );
    }

    fn video(id: u32, codec: &str, bandwidth: u64) -> MediaStream {
        MediaStream {
            id,
            base_url: format!("https://example.test/{id}/{bandwidth}"),
            codec: codec.to_string(),
            bandwidth,
        }
    }

    #[test]
    fn prefers_avc_within_requested_quality() {
        let streams = [
            video(80, "hev1.1.6.L120", 9_000_000),
            video(80, "avc1.640032", 4_000_000),
            video(64, "avc1.64001f", 2_000_000),
        ];
        assert_eq!(
            select_video_stream(&streams, 80).map(|stream| stream.codec.as_str()),
            Some("avc1.640032")
        );
    }

    #[test]
    fn falls_back_to_avc_below_requested_quality() {
        let streams = [
            video(80, "hev1.1.6.L120", 9_000_000),
            video(64, "avc1.64001f", 2_000_000),
        ];
        assert_eq!(
            select_video_stream(&streams, 80).map(|stream| stream.id),
            Some(64)
        );
    }

    #[test]
    fn pairs_quality_ids_with_descriptions() {
        assert_eq!(
            quality_options(&[80, 64, 32], &["1080P 高清", "720P 高清", "480P 清晰"]),
            vec![
                Quality {
                    id: 80,
                    label: "1080P 高清".to_string(),
                },
                Quality {
                    id: 64,
                    label: "720P 高清".to_string(),
                },
                Quality {
                    id: 32,
                    label: "480P 清晰".to_string(),
                },
            ]
        );
    }

    #[test]
    fn prefers_aac_audio() {
        let streams = [
            MediaStream {
                id: 30280,
                base_url: "https://example.test/ec3".to_string(),
                codec: "ec-3".to_string(),
                bandwidth: 640_000,
            },
            MediaStream {
                id: 30216,
                base_url: "https://example.test/aac".to_string(),
                codec: "mp4a.40.2".to_string(),
                bandwidth: 320_000,
            },
        ];
        assert_eq!(
            select_audio_stream(&streams).map(|stream| stream.id),
            Some(30216)
        );
    }

    #[test]
    fn parses_view_response_pages() {
        let info = parse_view_response(
            r#"{"code":0,"message":"OK","data":{"bvid":"BV1K2C2BSEZB","title":"Test","duration":1647,"pages":[{"page":1,"part":"Part A","duration":1647,"cid":34030291733}]}}"#,
        )
        .expect("valid view response");
        assert_eq!(
            info,
            VideoInfo {
                bvid: "BV1K2C2BSEZB".to_string(),
                title: "Test".to_string(),
                duration: 1647,
                pages: vec![VideoPage {
                    page: 1,
                    part: "Part A".to_string(),
                    duration: 1647,
                    cid: 34_030_291_733,
                }],
            }
        );
    }

    #[test]
    fn parses_target_season_page() {
        let page = parse_season_response(
            r#"{"code":0,"message":"OK","data":{"page":{"page_num":1,"page_size":20,"total":10},"meta":{"season_id":6490480,"name":"生成算法","total":10},"archives":[{"bvid":"BV1K2C2BSEZB","title":"One","duration":60}]}}"#,
            6_490_480,
        )
        .expect("valid season response");
        assert_eq!(
            page,
            SeasonPage {
                total: 10,
                archives: vec![SeasonArchive {
                    bvid: "BV1K2C2BSEZB".to_string(),
                    title: "One".to_string(),
                    duration: 60,
                }],
            }
        );
    }

    #[test]
    fn parses_direct_play_preview() {
        let play = parse_play_response(
            r#"{"code":0,"message":"OK","data":{"quality":80,"timelength":1646133,"accept_quality":[80,64],"accept_description":["1080P","720P"],"durl":[{"url":"https://cdn.test/video.mp4","length":179868,"size":3202987}]}}"#,
        )
        .expect("valid play response");
        assert_eq!(
            play,
            PlayData {
                qualities: vec![
                    Quality {
                        id: 80,
                        label: "1080P".to_string(),
                    },
                    Quality {
                        id: 64,
                        label: "720P".to_string(),
                    },
                ],
                videos: vec![],
                audios: vec![],
                direct_urls: vec!["https://cdn.test/video.mp4".to_string()],
                direct_size: Some(3_202_987),
                duration_ms: 1_646_133,
                media_duration_ms: 179_868,
            }
        );
        assert_eq!(play.media_duration_ms, 179_868);
    }

    #[test]
    fn parses_all_direct_video_segments_in_order() {
        let play = parse_play_response(
            r#"{"code":0,"data":{"accept_quality":[16],"accept_description":["360P"],"timelength":10000,"durl":[{"url":"https://cdn.test/0.mp4","length":6000,"size":10},{"url":"https://cdn.test/1.mp4","length":4000,"size":20}]}}"#,
        )
        .expect("segmented play response");
        assert_eq!(
            play.direct_urls,
            vec![
                "https://cdn.test/0.mp4".to_string(),
                "https://cdn.test/1.mp4".to_string()
            ]
        );
        assert_eq!(play.direct_size, Some(30));
        assert_eq!(play.media_duration_ms, 10_000);
    }

    #[test]
    fn classifies_logged_out_play_error() {
        assert_eq!(
            parse_play_response(r#"{"code":-101,"message":"账号未登录","data":null}"#),
            Err(ApiError::LoginRequired)
        );
    }

    #[test]
    fn classifies_entitlement_error() {
        assert_eq!(
            parse_play_response(r#"{"code":-403,"message":"大会员专享内容","data":null}"#),
            Err(ApiError::PermissionDenied("大会员专享内容".to_string()))
        );
    }

    #[test]
    fn accepts_only_official_qr_urls() {
        assert!(is_official_qr_url(
            "https://account.bilibili.com/h5/account-h5/auth/scan-web?qrcode_key=1"
        ));
        assert!(is_official_qr_url(
            "https://passport.bilibili.com/h5/account-h5/auth/scan-web?qrcode_key=1"
        ));
        assert!(!is_official_qr_url(
            "http://account.bilibili.com/h5/account-h5/auth/scan-web?qrcode_key=1"
        ));
        assert!(!is_official_qr_url(
            "https://account.bilibili.com.evil.test/h5/login"
        ));
    }

    #[test]
    fn parses_qr_generate_and_waiting_poll() {
        let login = parse_qr_generate(
            r#"{"code":0,"message":"OK","data":{"url":"https://account.bilibili.com/h5/account-h5/auth/scan-web?qrcode_key=abc","qrcode_key":"abc"}}"#,
        )
        .expect("valid QR response");
        assert_eq!(
            login,
            QrLogin {
                url: "https://account.bilibili.com/h5/account-h5/auth/scan-web?qrcode_key=abc"
                    .to_string(),
                key: "abc".to_string(),
            }
        );
        assert_eq!(
            parse_qr_poll(r#"{"code":0,"data":{"code":86101,"message":"未扫码"}}"#),
            Ok(QrState::Waiting)
        );
        assert_eq!(
            parse_qr_poll(r#"{"code":0,"data":{"code":86090,"message":"已扫码"}}"#),
            Ok(QrState::Scanned)
        );
        assert_eq!(
            parse_qr_poll(r#"{"code":0,"data":{"code":86038,"message":"已失效"}}"#),
            Ok(QrState::Expired)
        );
    }
}
