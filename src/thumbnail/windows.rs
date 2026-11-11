use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::SIZE,
        Graphics::Gdi::{
            BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS,
            DeleteDC, DeleteObject, GetDIBits, GetObjectW, HBITMAP, HDC,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY},
    },
    core::PCWSTR,
};

struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

struct DeviceContext(HDC);
impl Drop for DeviceContext {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

pub fn load(path: &Path) -> Result<(gpui_kit::ImageFormat, Vec<u8>), String> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }
        .map_err(|error| format!("无法初始化系统缩略图服务：{error}"))?;
    let _com = Com;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }
            .map_err(|error| format!("无法打开缩略图来源：{error}"))?;
    let bitmap = Bitmap(
        unsafe { factory.GetImage(SIZE { cx: 128, cy: 128 }, SIIGBF_THUMBNAILONLY) }
            .map_err(|error| format!("系统无法生成缩略图：{error}"))?,
    );
    let mut source = BITMAP::default();
    let read = unsafe {
        GetObjectW(
            bitmap.0.into(),
            size_of::<BITMAP>() as i32,
            Some((&mut source as *mut BITMAP).cast()),
        )
    };
    if read != size_of::<BITMAP>() as i32 || source.bmWidth <= 0 || source.bmHeight <= 0 {
        return Err("系统缩略图尺寸无效".into());
    }
    let width = source.bmWidth as u32;
    let height = source.bmHeight as u32;
    if width > 1024 || height > 1024 {
        return Err("系统缩略图尺寸过大".into());
    }
    let dc = DeviceContext(unsafe { CreateCompatibleDC(None) });
    if dc.0.is_invalid() {
        return Err("无法创建系统缩略图设备上下文".into());
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: height as i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let lines = unsafe {
        GetDIBits(
            dc.0,
            bitmap.0,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if lines != height as i32 {
        return Err("无法读取系统缩略图像素".into());
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    let size = 54u32 + pixels.len() as u32;
    let mut bytes = Vec::with_capacity(size as usize);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(width as i32).to_le_bytes());
    bytes.extend_from_slice(&(height as i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&[0; 24]);
    bytes.extend_from_slice(&pixels);
    Ok((gpui_kit::ImageFormat::Bmp, bytes))
}
