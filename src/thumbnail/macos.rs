use block2::RcBlock;
use objc2::{AnyThread, rc::autoreleasepool};
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_quick_look_thumbnailing::{
    QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes,
    QLThumbnailGenerator,
};
use std::{fs, path::Path, sync::mpsc, time::Duration};

pub fn load(path: &Path) -> Result<(gpui_kit::ImageFormat, Vec<u8>), String> {
    autoreleasepool(|_| {
        let directory =
            tempfile::tempdir().map_err(|error| format!("无法创建缩略图临时目录：{error}"))?;
        let output = directory.path().join("thumbnail.png");
        let source_url = NSURL::from_file_path(path).ok_or("缩略图来源路径无效")?;
        let output_url = NSURL::from_file_path(&output).ok_or("缩略图输出路径无效")?;
        let png_type = NSString::from_str("public.png");
        // URL 和请求对象在回调完成或取消前保持有效。
        let request = unsafe {
            QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
                QLThumbnailGenerationRequest::alloc(),
                &source_url,
                CGSize::new(128.0, 128.0),
                1.0,
                QLThumbnailGenerationRequestRepresentationTypes::Thumbnail,
            )
        };
        let generator = unsafe { QLThumbnailGenerator::sharedGenerator() };
        let (sender, receiver) = mpsc::channel();
        let callback = RcBlock::new(move |error: *mut NSError| {
            let result = autoreleasepool(|_| {
                // 系统仅在回调期间保证错误对象有效。
                unsafe { error.as_ref() }
                    .map(|error| format!("系统无法生成缩略图：{}", error.localizedDescription()))
                    .map_or(Ok(()), Err)
            });
            let _ = sender.send(result);
        });
        #[allow(deprecated)]
        unsafe {
            generator
                .saveBestRepresentationForRequest_toFileAtURL_withContentType_completionHandler(
                    &request,
                    &output_url,
                    &png_type,
                    &callback,
                )
        };
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => result?,
            Err(error) => {
                unsafe { generator.cancelRequest(&request) };
                return Err(format!("系统缩略图生成未完成：{error}"));
            }
        }
        let bytes = fs::read(&output).map_err(|error| format!("无法读取系统缩略图：{error}"))?;
        Ok((gpui_kit::ImageFormat::Png, bytes))
    })
}
