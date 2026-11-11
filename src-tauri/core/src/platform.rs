use crate::model::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::{
    ffi::{CStr, CString, OsString},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
        io::{AsRawFd, FromRawFd, OwnedFd},
    },
    path::{Path, PathBuf},
};

pub fn path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().as_bytes().to_vec()
}
pub fn from_bytes(bytes: Vec<u8>) -> PathBuf {
    OsString::from_vec(bytes).into()
}
pub fn scope(path: PathBuf) -> Result<Scope> {
    let path = std::fs::canonicalize(path)?;
    if path == Path::new("/") || path == Path::new("/System/Volumes/Data") {
        return Err(AppError::new(
            "scope_invalid",
            "首版不支持直接扫描整个启动卷，请选择用户目录或具体文件夹。",
        ));
    }
    let m = std::fs::metadata(&path)?;
    if !m.is_dir() {
        return Err(AppError::new("scope_invalid", "请选择文件夹。"));
    }
    #[cfg(target_os = "macos")]
    {
        let c = CString::new(path_bytes(&path))
            .map_err(|_| AppError::new("scope_invalid", "无效路径"))?;
        let mut fs = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::statfs(c.as_ptr(), fs.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let fs = unsafe { fs.assume_init() };
        if fs.f_flags & libc::MNT_LOCAL as u32 == 0 {
            return Err(AppError::new(
                "scope_invalid",
                "首版仅支持本地磁盘，不扫描网络共享。",
            ));
        }
        if fs.f_flags & libc::MNT_SNAPSHOT as u32 != 0 || path.starts_with("/Volumes/.timemachine")
        {
            return Err(AppError::new(
                "scope_invalid",
                "首版不扫描挂载快照，请选择当前文件目录。",
            ));
        }
    }
    Ok(Scope {
        id: uuid::Uuid::new_v4().to_string(),
        key: format!(
            "{}:{}:{}",
            m.dev(),
            m.ino(),
            URL_SAFE_NO_PAD.encode(path_bytes(&path))
        ),
        path_display: path.to_string_lossy().into(),
        available: true,
        path,
        device: m.dev(),
        inode: m.ino(),
    })
}
pub fn check_scope(s: &Scope) -> Result<()> {
    let m = std::fs::symlink_metadata(&s.path)
        .map_err(|_| AppError::new("scope_invalid", "扫描范围已失效或无法访问，请重新选择。"))?;
    if !m.is_dir() || m.dev() != s.device || m.ino() != s.inode {
        return Err(AppError::new(
            "scope_invalid",
            "扫描范围的身份已经变化，请重新选择。",
        ));
    }
    Ok(())
}
pub fn volume(path: &Path, is_home: bool) -> Result<Volume> {
    let c =
        CString::new(path_bytes(path)).map_err(|_| AppError::new("scope_invalid", "无效路径"))?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let stat = unsafe { stat.assume_init() };
    let unit = stat.f_frsize as u64;
    let bytes = |v: u64| -> Result<u64> {
        v.checked_mul(unit)
            .ok_or_else(|| AppError::new("size_overflow", "卷容量超出可表示范围。"))
    };
    let total = bytes(stat.f_blocks as u64)?;
    let free = bytes(stat.f_bfree as u64)?;
    let available = bytes(stat.f_bavail as u64)?;
    let m = std::fs::metadata(path)?;
    Ok(Volume {
        id: m.dev().to_string(),
        name: if is_home {
            "本机数据卷".into()
        } else {
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into()
        },
        path_display: path.to_string_lossy().into(),
        total: total.to_string(),
        free: free.to_string(),
        available: available.to_string(),
        used: total.saturating_sub(free).to_string(),
        sampled_at: now(),
        is_home,
    })
}
pub fn volumes(home: &Path) -> Vec<Volume> {
    let mut found = Vec::new();
    if let Ok(v) = volume(home, true) {
        found.push(v);
    }
    #[cfg(target_os = "macos")]
    unsafe {
        let mut ptr: *mut libc::statfs = std::ptr::null_mut();
        let count = libc::getmntinfo(&mut ptr, libc::MNT_NOWAIT);
        if count > 0 && !ptr.is_null() {
            for m in std::slice::from_raw_parts(ptr, count as usize) {
                if m.f_flags & libc::MNT_LOCAL as u32 == 0
                    || m.f_flags & libc::MNT_SNAPSHOT as u32 != 0
                {
                    continue;
                }
                let p = from_bytes(CStr::from_ptr(m.f_mntonname.as_ptr()).to_bytes().to_vec());
                // Snapshot mounts live beneath hidden directories such as
                // /Volumes/.timemachine; they are not independent disks.
                if !is_visible_volume_mount(&p) {
                    continue;
                }
                if let Ok(v) = volume(&p, false) {
                    if !found.iter().any(|old| old.id == v.id) {
                        found.push(v);
                    }
                }
            }
        }
    }
    found
}

fn is_visible_volume_mount(path: &Path) -> bool {
    path.parent() == Some(Path::new("/Volumes"))
        && path
            .file_name()
            .is_some_and(|name| !name.as_bytes().starts_with(b"."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounted_snapshots_are_not_independent_volumes() {
        assert!(is_visible_volume_mount(Path::new("/Volumes/External SSD")));
        for path in [
            "/Volumes/.timemachine",
            "/Volumes/.timemachine/id/date.backup",
            "/Volumes/External SSD/nested",
            "/System/Volumes/Data",
            "/Volumes",
        ] {
            assert!(!is_visible_volume_mount(Path::new(path)), "{path}");
        }
    }
}

pub struct Directory {
    fd: OwnedFd,
    stream: *mut libc::DIR,
}
impl Directory {
    pub fn root(scope: &Scope) -> Result<Self> {
        let c = CString::new(path_bytes(&scope.path))
            .map_err(|_| AppError::new("scope_invalid", "无效路径"))?;
        let fd = unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let dir = Self::from_fd(fd)?;
        let meta = dir.metadata()?;
        if meta.st_dev as u64 != scope.device || meta.st_ino as u64 != scope.inode {
            return Err(AppError::new("entry_replaced", "根目录身份发生变化。"));
        }
        Ok(dir)
    }
    fn from_fd(fd: i32) -> Result<Self> {
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let duplicate = unsafe { libc::dup(fd.as_raw_fd()) };
        if duplicate < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let stream = unsafe { libc::fdopendir(duplicate) };
        if stream.is_null() {
            unsafe { libc::close(duplicate) };
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self { fd, stream })
    }
    pub fn metadata(&self) -> Result<libc::stat> {
        let mut s = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(self.fd.as_raw_fd(), s.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { s.assume_init() })
    }
    pub fn child_metadata(&self, name: &CStr) -> Result<libc::stat> {
        let mut s = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe {
            libc::fstatat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                s.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { s.assume_init() })
    }
    pub fn child(&self, name: &CStr, expected: &libc::stat) -> Result<Self> {
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        let dir = Self::from_fd(fd)?;
        let m = dir.metadata()?;
        if m.st_dev != expected.st_dev || m.st_ino != expected.st_ino {
            return Err(AppError::new("entry_replaced", "目录在扫描中被替换。"));
        }
        Ok(dir)
    }
    pub fn next_name(&mut self) -> Result<Option<CString>> {
        loop {
            unsafe {
                *errno_ptr() = 0;
            }
            let item = unsafe { libc::readdir(self.stream) };
            if item.is_null() {
                let e = unsafe { *errno_ptr() };
                return if e == 0 {
                    Ok(None)
                } else {
                    Err(std::io::Error::from_raw_os_error(e).into())
                };
            }
            let n = unsafe { CStr::from_ptr((*item).d_name.as_ptr()) };
            if n.to_bytes() != b"." && n.to_bytes() != b".." {
                return Ok(Some(n.to_owned()));
            }
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        unsafe {
            libc::closedir(self.stream);
        }
    }
}
#[cfg(target_os = "macos")]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__error()
}
#[cfg(not(target_os = "macos"))]
unsafe fn errno_ptr() -> *mut libc::c_int {
    libc::__errno_location()
}

pub fn verified_path(root: &Path, raw: Vec<u8>, expected: Option<(u64, u64)>) -> Result<PathBuf> {
    let path = from_bytes(raw);
    if !path.starts_with(root) {
        return Err(AppError::new("scope_invalid", "条目不属于扫描范围。"));
    }
    let m = std::fs::symlink_metadata(&path)
        .map_err(|_| AppError::new("entry_missing", "项目已消失或磁盘离线，请重新扫描。"))?;
    if let Some((dev, ino)) = expected {
        if m.dev() != dev || m.ino() != ino {
            return Err(AppError::new(
                "entry_replaced",
                "原路径已被其他项目替换，请重新扫描。",
            ));
        }
    }
    // Check ancestor resolution, while allowing Finder to select a symlink itself.
    let parent = path.parent().unwrap_or(root);
    let canonical_parent = std::fs::canonicalize(parent)?;
    if path != root && !canonical_parent.starts_with(root) {
        return Err(AppError::new(
            "scope_invalid",
            "项目的父目录已指向扫描范围之外。",
        ));
    }
    Ok(path)
}
