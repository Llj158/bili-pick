use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

pub fn session_path() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|root| PathBuf::from(root).join("BiliPick").join("session.dat"))
        .ok_or_else(|| "找不到当前用户的 LocalAppData 目录".to_string())
}

pub fn load() -> Result<Option<String>, String> {
    let path = session_path()?;
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法读取登录记录：{error}")),
    };
    if metadata.len() > 64 * 1024 {
        return Err("登录记录大小异常，请取消“记住登录”后重新扫码".to_string());
    }
    let mut encrypted =
        std::fs::read(path).map_err(|error| format!("读取登录记录失败：{error}"))?;
    let mut decrypted = protect(&mut encrypted, false)?;
    let result = std::str::from_utf8(&decrypted)
        .map(str::to_owned)
        .map_err(|_| "登录记录内容无效，请重新扫码".to_string());
    decrypted.fill(0);
    result.map(Some)
}

pub fn save(cookie: &str) -> Result<(), String> {
    if cookie.is_empty() {
        return Err("当前没有可以保存的登录记录".to_string());
    }
    let path = session_path()?;
    let mut plain = cookie.as_bytes().to_vec();
    let encrypted = protect(&mut plain, true);
    plain.fill(0);
    let encrypted = encrypted?;
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|error| format!("无法创建登录记录目录：{error}"))?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, encrypted)
        .map_err(|error| format!("无法写入加密登录记录：{error}"))?;
    let source: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        let error = io::Error::last_os_error();
        let _ = std::fs::remove_file(temporary);
        return Err(format!("无法替换登录记录：{error}"));
    }
    Ok(())
}

pub fn forget() -> Result<(), String> {
    match std::fs::remove_file(session_path()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法删除登录记录：{error}")),
    }
}

fn protect(bytes: &mut [u8], encrypt: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err(format!(
            "Windows 登录记录{}失败：{}",
            if encrypt { "加密" } else { "解密" },
            io::Error::last_os_error()
        ));
    }
    let mut result = Vec::new();
    if !output.pbData.is_null() {
        unsafe {
            result.extend_from_slice(std::slice::from_raw_parts(
                output.pbData,
                output.cbData as usize,
            ));
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
            LocalFree(output.pbData.cast());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::protect;

    #[test]
    fn saves_replaces_and_forgets_session_in_isolated_profile() {
        if std::env::var_os("BILIPICK_SESSION_TEST").is_some() {
            assert_eq!(super::load().unwrap(), None);
            super::save("SESSDATA=first-secret").unwrap();
            assert_eq!(
                super::load().unwrap().as_deref(),
                Some("SESSDATA=first-secret")
            );
            let path = super::session_path().unwrap();
            let encrypted = std::fs::read(&path).unwrap();
            assert!(!String::from_utf8_lossy(&encrypted).contains("first-secret"));
            super::save("SESSDATA=second-secret").unwrap();
            assert_eq!(
                super::load().unwrap().as_deref(),
                Some("SESSDATA=second-secret")
            );
            assert_eq!(
                std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
                1
            );
            std::fs::write(&path, b"corrupted-record").unwrap();
            assert!(super::load().is_err());
            super::forget().unwrap();
            assert!(!path.exists());
            super::forget().unwrap();
            return;
        }
        let root =
            std::env::temp_dir().join(format!("bilipick-session-test-{}", std::process::id()));
        assert!(!root.exists(), "test profile must be a new directory");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "session::tests::saves_replaces_and_forgets_session_in_isolated_profile",
                "--nocapture",
            ])
            .env("LOCALAPPDATA", &root)
            .env("BILIPICK_SESSION_TEST", "1")
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::remove_dir(root.join("BiliPick")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn dpapi_roundtrip_hides_cookie_and_rejects_modified_data() {
        let cookie = b"SESSDATA=bilipick-test-secret; bili_jct=test-csrf";
        let mut encrypted = protect(&mut cookie.to_vec(), true).unwrap();
        assert!(!encrypted.windows(20).any(|chunk| chunk == &cookie[..20]));
        assert_eq!(protect(&mut encrypted, false).unwrap(), cookie);
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xff;
        assert!(protect(&mut encrypted, false).is_err());
        assert!(protect(&mut [], false).is_err());
    }
}
