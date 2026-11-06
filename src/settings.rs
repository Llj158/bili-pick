use std::io;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub output_directory: PathBuf,
    pub remember_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_directory: std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Videos")
                .join("BiliPick"),
            remember_login: false,
        }
    }
}

impl Settings {
    pub fn load() -> Result<Self, String> {
        let path = crate::session::session_path()?.with_file_name("settings.json");
        let raw = match std::fs::read(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Self {
                    remember_login: crate::session::session_path()?.exists(),
                    ..Self::default()
                });
            }
            Err(error) => return Err(format!("读取设置失败：{error}")),
        };
        let value: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|error| format!("设置文件格式无效：{error}"))?;
        if !value.is_object() {
            return Err("设置文件必须是 JSON 对象".to_string());
        }
        let mut settings = Self::default();
        if let Some(output) = value.get("output_directory") {
            let output = output
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "设置中的保存目录无效".to_string())?;
            settings.output_directory = PathBuf::from(output);
        }
        if let Some(remember) = value.get("remember_login") {
            settings.remember_login = remember
                .as_bool()
                .ok_or_else(|| "设置中的记住登录选项无效".to_string())?;
        }
        Ok(settings)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = crate::session::session_path()?.with_file_name("settings.json");
        std::fs::create_dir_all(path.parent().unwrap())
            .map_err(|error| format!("创建设置目录失败：{error}"))?;
        let value = serde_json::json!({
            "output_directory": self.output_directory.to_string_lossy(),
            "remember_login": self.remember_login,
        });
        std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap())
            .map_err(|error| format!("保存设置失败：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn persists_preferences_separately_from_credentials_in_isolated_profile() {
        if std::env::var_os("BILIPICK_SETTINGS_TEST").is_some() {
            let initial = Settings::load().unwrap();
            assert_eq!(initial, Settings::default());
            crate::session::save("SESSDATA=migration-fixture").unwrap();
            assert!(Settings::load().unwrap().remember_login);

            let mut settings = Settings {
                output_directory: std::env::temp_dir()
                    .join("哔取 视频文件夹")
                    .join("新建目录"),
                remember_login: true,
            };
            settings.save().unwrap();
            assert_eq!(Settings::load().unwrap(), settings);
            crate::session::forget().unwrap();
            assert_eq!(
                Settings::load().unwrap(),
                settings,
                "logout keeps preferences"
            );
            settings.remember_login = false;
            settings.save().unwrap();
            assert_eq!(Settings::load().unwrap(), settings);

            let path = crate::session::session_path()
                .unwrap()
                .with_file_name("settings.json");
            let json = std::fs::read_to_string(&path).unwrap();
            assert!(!json.contains("SESSDATA"));
            std::fs::write(&path, r#"{"remember_login":true,"future_setting":1}"#).unwrap();
            assert_eq!(
                Settings::load().unwrap().output_directory,
                initial.output_directory
            );
            for invalid in [
                r#"{"output_directory":""}"#,
                r#"{"remember_login":"yes"}"#,
                "[]",
                "broken",
            ] {
                std::fs::write(&path, invalid).unwrap();
                assert!(Settings::load().is_err());
            }
            std::fs::remove_file(path).unwrap();
            return;
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("bilipick-settings-{stamp}"));
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "settings::tests::persists_preferences_separately_from_credentials_in_isolated_profile", "--nocapture"])
            .env("LOCALAPPDATA", &root)
            .env("BILIPICK_SETTINGS_TEST", "1")
            .status().unwrap();
        assert!(status.success());
        std::fs::remove_dir(root.join("BiliPick")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
