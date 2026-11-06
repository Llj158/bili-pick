use crate::bilibili::{ApiError, select_audio_stream, select_video_stream};
use crate::http::HttpError;
use crate::service::BiliClient;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Write, copy};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadItem {
    pub bvid: String,
    pub title: String,
    pub part: String,
    pub page: u32,
    pub cid: u64,
    pub duration: u64,
    pub sequence: u32,
}

#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

#[derive(Debug)]
pub enum DownloadError {
    Api(ApiError),
    Http(HttpError),
    Io(String),
    Tools(String),
    Media(String),
    Preview { actual: f64, expected: f64 },
    Cancelled,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api(error) => write!(formatter, "{error}"),
            Self::Http(error) => write!(formatter, "{error}"),
            Self::Io(message) | Self::Tools(message) | Self::Media(message) => {
                formatter.write_str(message)
            }
            Self::Preview { actual, expected } => write!(
                formatter,
                "下载内容只有 {actual:.1} 秒，但视频标注为 {expected:.0} 秒，可能是预览片段"
            ),
            Self::Cancelled => formatter.write_str("已取消"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<ApiError> for DownloadError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

impl From<HttpError> for DownloadError {
    fn from(error: HttpError) -> Self {
        match error {
            HttpError::Cancelled => Self::Cancelled,
            other => Self::Http(other),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DownloadEvent {
    Status(String),
    Progress { current: u64, total: Option<u64> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadResult {
    pub path: PathBuf,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeInfo {
    pub duration_secs: f64,
    pub has_video: bool,
    pub has_audio: bool,
}

pub fn sanitize_filename(value: &str) -> String {
    let mut result: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    result = result.trim_end_matches([' ', '.']).to_string();
    if result.is_empty() {
        "未命名视频".to_string()
    } else {
        result
    }
}

pub fn is_short_preview(actual_secs: f64, expected_secs: f64) -> bool {
    let tolerance = (expected_secs * 0.03).max(5.0);
    actual_secs + tolerance < expected_secs
}

pub fn parse_probe_output(input: &str) -> Result<ProbeInfo, String> {
    let root: serde_json::Value =
        serde_json::from_str(input).map_err(|error| format!("无法解析 ffprobe 输出: {error}"))?;
    let duration_secs = root
        .pointer("/format/duration")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "ffprobe 输出缺少时长".to_string())?
        .parse()
        .map_err(|_| "ffprobe 时长格式无效".to_string())?;
    let streams = root
        .get("streams")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "ffprobe 输出缺少流信息".to_string())?;
    Ok(ProbeInfo {
        duration_secs,
        has_video: streams.iter().any(|stream| {
            stream.get("codec_type").and_then(serde_json::Value::as_str) == Some("video")
        }),
        has_audio: streams.iter().any(|stream| {
            stream.get("codec_type").and_then(serde_json::Value::as_str) == Some("audio")
        }),
    })
}

pub fn unique_path(directory: &Path, stem: &str, extension: &str) -> PathBuf {
    let initial = directory.join(format!("{stem}.{extension}"));
    if !initial.exists() {
        return initial;
    }
    for number in 2.. {
        let candidate = directory.join(format!("{stem} ({number}).{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("integer suffix search is unbounded")
}

pub fn discover_tools(explicit: Option<&str>) -> Result<Tools, DownloadError> {
    let explicit = explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("BILIPICK_FFMPEG")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        });
    if let Some(path) = explicit {
        let directory = if path.is_dir() {
            path
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        };
        let ffmpeg = directory.join("ffmpeg.exe");
        let ffprobe = directory.join("ffprobe.exe");
        if ffmpeg.is_file() && ffprobe.is_file() {
            return Ok(Tools { ffmpeg, ffprobe });
        }
        return Err(DownloadError::Tools(format!(
            "指定目录中缺少 ffmpeg.exe 或 ffprobe.exe：{}",
            directory.display()
        )));
    }

    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let ffmpeg = directory.join("ffmpeg.exe");
        let ffprobe = directory.join("ffprobe.exe");
        if ffmpeg.is_file() && ffprobe.is_file() {
            return Ok(Tools { ffmpeg, ffprobe });
        }
    }
    Err(DownloadError::Tools(
        "未找到 ffmpeg.exe 和 ffprobe.exe，请在设置中指定它们所在目录".to_string(),
    ))
}

pub fn download_item(
    client: &mut BiliClient,
    item: &DownloadItem,
    quality: u32,
    output_directory: &Path,
    tools: &Tools,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DownloadEvent),
) -> Result<DownloadResult, DownloadError> {
    fs::create_dir_all(output_directory)
        .map_err(|error| DownloadError::Io(format!("无法创建保存目录: {error}")))?;
    progress(DownloadEvent::Status("正在获取播放地址".to_string()));
    let play = client.play(&item.bvid, item.cid, quality)?;
    if play.media_duration_ms > 0
        && is_short_preview(play.media_duration_ms as f64 / 1000.0, item.duration as f64)
    {
        if !client.is_logged_in() {
            return Err(DownloadError::Api(ApiError::LoginRequired));
        }
        return Err(DownloadError::Media(format!(
            "登录账号仍只返回短预览（{:.1} 秒），请检查大会员、充电或其他访问权限",
            play.media_duration_ms as f64 / 1000.0
        )));
    }

    let temp_stem = format!(".bilipick-{}-{}", item.bvid, item.cid);
    let direct_path = output_directory.join(format!("{temp_stem}.direct.part"));
    let video_path = output_directory.join(format!("{temp_stem}.video.part"));
    let audio_path = output_directory.join(format!("{temp_stem}.audio.part"));
    let merged_path = output_directory.join(format!("{temp_stem}.merged.part.mp4"));

    let ready_file = if !play.direct_urls.is_empty() {
        if play.direct_urls.len() == 1 {
            download_bytes(
                client,
                &play.direct_urls[0],
                &direct_path,
                "视频",
                cancel,
                progress,
            )?;
        } else {
            let mut parts = Vec::with_capacity(play.direct_urls.len());
            for (index, url) in play.direct_urls.iter().enumerate() {
                let part = output_directory.join(format!("{temp_stem}.direct.{index}.part"));
                download_bytes(client, url, &part, "视频分段", cancel, progress)?;
                parts.push(part);
            }
            progress(DownloadEvent::Status("正在拼接视频分段".to_string()));
            concatenate_parts(&parts, &direct_path)?;
            for part in parts {
                let _ = fs::remove_file(part);
            }
        }
        direct_path
    } else {
        let video = select_video_stream(&play.videos, quality)
            .ok_or_else(|| DownloadError::Media("接口没有返回可用视频流".to_string()))?;
        let audio = select_audio_stream(&play.audios)
            .ok_or_else(|| DownloadError::Media("接口没有返回可用音频流".to_string()))?;
        download_bytes(
            client,
            &video.base_url,
            &video_path,
            "视频",
            cancel,
            progress,
        )?;
        download_bytes(
            client,
            &audio.base_url,
            &audio_path,
            "音频",
            cancel,
            progress,
        )?;
        progress(DownloadEvent::Status("正在合并音视频".to_string()));
        let _ = fs::remove_file(&merged_path);
        run_ffmpeg(
            &tools.ffmpeg,
            &video_path,
            &audio_path,
            &merged_path,
            cancel,
        )?;
        merged_path
    };

    progress(DownloadEvent::Status("正在校验媒体文件".to_string()));
    let probe = run_ffprobe(&tools.ffprobe, &ready_file)?;
    if !probe.has_video {
        let _ = fs::remove_file(&ready_file);
        return Err(DownloadError::Media("下载结果没有视频轨".to_string()));
    }
    if !probe.has_audio {
        let _ = fs::remove_file(&ready_file);
        return Err(DownloadError::Media("下载结果没有音频轨".to_string()));
    }
    if item.duration > 0 && is_short_preview(probe.duration_secs, item.duration as f64) {
        let _ = fs::remove_file(&ready_file);
        return Err(DownloadError::Preview {
            actual: probe.duration_secs,
            expected: item.duration as f64,
        });
    }

    let final_path = unique_path(output_directory, &output_stem(item), "mp4");
    fs::rename(&ready_file, &final_path)
        .map_err(|error| DownloadError::Io(format!("无法写入最终文件: {error}")))?;
    let _ = fs::remove_file(&video_path);
    let _ = fs::remove_file(&audio_path);
    append_manifest(
        output_directory,
        item,
        &final_path,
        probe.duration_secs,
        "完成",
    );
    Ok(DownloadResult {
        path: final_path,
        duration_secs: probe.duration_secs,
    })
}

fn concatenate_parts(parts: &[PathBuf], output: &Path) -> Result<(), DownloadError> {
    let mut destination = fs::File::create(output)
        .map_err(|error| DownloadError::Io(format!("无法创建分段合并文件: {error}")))?;
    for part in parts {
        let mut source = fs::File::open(part)
            .map_err(|error| DownloadError::Io(format!("无法读取视频分段: {error}")))?;
        copy(&mut source, &mut destination)
            .map_err(|error| DownloadError::Io(format!("无法拼接视频分段: {error}")))?;
    }
    destination
        .flush()
        .map_err(|error| DownloadError::Io(format!("无法刷新分段合并文件: {error}")))
}

fn download_bytes(
    client: &mut BiliClient,
    url: &str,
    path: &Path,
    label: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(DownloadEvent),
) -> Result<(), DownloadError> {
    progress(DownloadEvent::Status(format!("正在下载{label}")));
    client.download_to(url, path, cancel, |current, total| {
        progress(DownloadEvent::Progress { current, total });
    })?;
    Ok(())
}

fn output_stem(item: &DownloadItem) -> String {
    let content = if item.page > 1 {
        format!("{} - P{:02} {}", item.title, item.page, item.part)
    } else {
        item.title.clone()
    };
    sanitize_filename(&format!(
        "{:03} - {} - {}",
        item.sequence, item.bvid, content
    ))
}

fn run_ffmpeg(
    ffmpeg: &Path,
    video: &Path,
    audio: &Path,
    output: &Path,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(audio)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("1:a:0")
        .arg("-c")
        .arg("copy")
        .arg("-movflags")
        .arg("+faststart")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|error| DownloadError::Tools(format!("无法启动 ffmpeg: {error}")))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DownloadError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| DownloadError::Io(format!("读取 ffmpeg 结果失败: {error}")))?;
                if !status.success() {
                    return Err(DownloadError::Media(format!(
                        "ffmpeg 合并失败: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    )));
                }
                return Ok(());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(DownloadError::Io(format!("等待 ffmpeg 失败: {error}"))),
        }
    }
}

fn run_ffprobe(ffprobe: &Path, path: &Path) -> Result<ProbeInfo, DownloadError> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|error| DownloadError::Tools(format!("无法启动 ffprobe: {error}")))?;
    if !output.status.success() {
        return Err(DownloadError::Media(format!(
            "ffprobe 校验失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    parse_probe_output(&String::from_utf8_lossy(&output.stdout)).map_err(DownloadError::Media)
}

fn append_manifest(
    directory: &Path,
    item: &DownloadItem,
    path: &Path,
    duration_secs: f64,
    status: &str,
) {
    let manifest = directory.join("下载清单.txt");
    let line = format!(
        "{}\tP{}\t{}\t{:.1}s\t{}\t{}\r\n",
        item.bvid,
        item.page,
        item.title.replace(['\t', '\r', '\n'], " "),
        duration_secs,
        status,
        path.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
    );
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(manifest) {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadError, DownloadItem, discover_tools, download_item, is_short_preview,
        parse_probe_output, sanitize_filename, unique_path,
    };
    use crate::bilibili::ApiError;
    use crate::service::BiliClient;
    use std::fs;
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn removes_windows_illegal_filename_characters() {
        assert_eq!(
            sanitize_filename(r#"A<B>C:D"E/F\G|H?I*J. "#),
            "A_B_C_D_E_F_G_H_I_J"
        );
    }

    #[test]
    fn detects_short_preview_with_tolerance() {
        assert!(is_short_preview(180.0, 1647.0));
        assert!(!is_short_preview(1642.0, 1647.0));
    }

    #[test]
    fn parses_ffprobe_duration_and_stream_types() {
        let probe = parse_probe_output(
            r#"{"streams":[{"codec_type":"video"},{"codec_type":"audio"}],"format":{"duration":"1646.130000"}}"#,
        )
        .expect("valid ffprobe output");
        assert!((probe.duration_secs - 1646.13).abs() < 0.001);
        assert!(probe.has_video);
        assert!(probe.has_audio);
    }

    #[test]
    fn chooses_non_conflicting_output_name() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("bilipick-name-{}-{stamp}", std::process::id()));
        fs::create_dir(&path).expect("create temp directory");
        fs::write(path.join("video.mp4"), b"existing").expect("seed existing file");
        assert_eq!(
            unique_path(&path, "video", "mp4"),
            path.join("video (2).mp4")
        );
        let _ = fs::remove_file(path.join("video.mp4"));
        let _ = fs::remove_dir(path);
    }

    #[test]
    #[ignore = "downloads a public preview from the live Bilibili service"]
    fn rejects_real_public_preview_instead_of_marking_it_complete() {
        let output = std::env::temp_dir().join(format!("bilipick-preview-{}", std::process::id()));
        fs::create_dir_all(&output).expect("create preview test directory");
        let tools = discover_tools(None).expect("ffmpeg tools");
        let item = DownloadItem {
            bvid: "BV1K2C2BSEZB".to_string(),
            title: "FlowMatching".to_string(),
            part: "FlowMatching".to_string(),
            page: 1,
            cid: 34_030_291_733,
            duration: 1647,
            sequence: 1,
        };
        let mut client = BiliClient::default();
        let cancel = AtomicBool::new(false);
        let result = download_item(
            &mut client,
            &item,
            16,
            &output,
            &tools,
            &cancel,
            &mut |_| {},
        );
        assert!(matches!(
            result,
            Err(DownloadError::Api(ApiError::LoginRequired)) | Err(DownloadError::Preview { .. })
        ));
        let _ = fs::remove_dir_all(output);
    }
}
