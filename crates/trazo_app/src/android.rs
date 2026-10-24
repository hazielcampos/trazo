//! Android-only helpers through JNI: runtime permissions.
//!
//! Every call reports Java exceptions to logcat (`exception_describe`) and
//! clears them, so a failing call never poisons the next JNI call.

#![cfg(target_os = "android")]

use jni::objects::{JObject, JValue};
use jni::JNIEnv;

pub const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";
pub const READ_MEDIA_AUDIO: &str = "android.permission.READ_MEDIA_AUDIO";
pub const READ_MEDIA_IMAGES: &str = "android.permission.READ_MEDIA_IMAGES";
pub const READ_MEDIA_VIDEO: &str = "android.permission.READ_MEDIA_VIDEO";
pub const READ_EXTERNAL_STORAGE: &str = "android.permission.READ_EXTERNAL_STORAGE";

use std::sync::OnceLock;

/// The NativeActivity (`android.app.Activity`) as a JNI global reference
/// owned by the activity glue. `ndk_context` only exposes the Application,
/// which has no `requestPermissions`.
static ACTIVITY: OnceLock<usize> = OnceLock::new();

pub fn set_activity(ptr: *mut std::ffi::c_void) {
    let _ = ACTIVITY.set(ptr as usize);
}

fn with_activity<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>) -> Result<R, String> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let raw = ACTIVITY.get().copied().unwrap_or(ctx.context() as usize);
    let activity = unsafe { JObject::from_raw(raw as jni::sys::jobject) };
    // A local frame frees every local reference the call creates (the UI
    // thread stays attached, so they would otherwise pile up).
    let result = env.with_local_frame(64, |env| f(env, &activity));
    match result {
        Ok(r) => Ok(r),
        Err(e) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            Err(e.to_string())
        }
    }
}

/// Android API level of the device.
pub fn sdk_int() -> i32 {
    with_activity(|env, _| env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()).unwrap_or(0)
}

/// System language as a tag like "es-MX".
pub fn locale() -> Option<String> {
    with_activity(|env, _| {
        let loc = env.call_static_method("java/util/Locale", "getDefault", "()Ljava/util/Locale;", &[])?.l()?;
        let tag = env.call_method(&loc, "toLanguageTag", "()Ljava/lang/String;", &[])?.l()?;
        let tag: String = env.get_string(&jni::objects::JString::from(tag))?.into();
        Ok(tag)
    })
    .ok()
}

pub fn has_permission(name: &str) -> bool {
    with_activity(|env, activity| {
        let perm = env.new_string(name)?;
        env.call_method(activity, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&perm)])?.i()
    })
    .map(|r| r == 0)
    .unwrap_or(false)
}

/// Show the system permission dialog. The answer arrives asynchronously:
/// poll [`has_permission`] afterwards.
pub fn request_permissions(names: &[&str]) -> Result<(), String> {
    with_activity(|env, activity| {
        let string_class = env.find_class("java/lang/String")?;
        let array = env.new_object_array(names.len() as i32, &string_class, JObject::null())?;
        for (i, n) in names.iter().enumerate() {
            let s = env.new_string(n)?;
            env.set_object_array_element(&array, i as i32, &s)?;
        }
        env.call_method(activity, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&array), JValue::Int(7)])?;
        Ok(())
    })
}

/// Permissions needed to read pictures and sounds from shared storage.
pub fn media_permissions() -> Vec<&'static str> {
    if sdk_int() >= 33 {
        vec![READ_MEDIA_AUDIO, READ_MEDIA_IMAGES, READ_MEDIA_VIDEO]
    } else {
        vec![READ_EXTERNAL_STORAGE]
    }
}

/// Where an exported file is published in shared storage.
#[derive(Clone, Copy)]
pub enum SharedKind {
    Video,
    Image,
    Audio,
    Download,
}

impl SharedKind {
    fn collection(self) -> (&'static str, &'static str) {
        match self {
            SharedKind::Video => ("android/provider/MediaStore$Video$Media", "Movies/Trazo"),
            SharedKind::Image => ("android/provider/MediaStore$Images$Media", "Pictures/Trazo"),
            SharedKind::Audio => ("android/provider/MediaStore$Audio$Media", "Music/Trazo"),
            SharedKind::Download => ("android/provider/MediaStore$Downloads", "Download/Trazo"),
        }
    }

    pub fn folder(self) -> &'static str {
        self.collection().1
    }
}

/// Copy `path` into the gallery / Downloads through MediaStore (Android 10+,
/// no permission needed). Returns the content URI as a string.
pub fn save_to_shared(path: &std::path::Path, name: &str, mime: &str, kind: SharedKind) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    with_activity(|env, activity| {
        let resolver = env.call_method(activity, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
        let values = env.new_object("android/content/ContentValues", "()V", &[])?;
        for (k, v) in [("_display_name", name), ("mime_type", mime), ("relative_path", kind.collection().1)] {
            let (k, v) = (env.new_string(k)?, env.new_string(v)?);
            env.call_method(&values, "put", "(Ljava/lang/String;Ljava/lang/String;)V", &[JValue::Object(&k), JValue::Object(&v)])?;
        }
        // Hidden from other apps until it is complete.
        let pending = env.new_string("is_pending")?;
        let one = env.new_object("java/lang/Integer", "(I)V", &[JValue::Int(1)])?;
        env.call_method(&values, "put", "(Ljava/lang/String;Ljava/lang/Integer;)V", &[JValue::Object(&pending), JValue::Object(&one)])?;
        let class = env.find_class(kind.collection().0)?;
        let collection = env.get_static_field(&class, "EXTERNAL_CONTENT_URI", "Landroid/net/Uri;")?.l()?;
        let uri = env
            .call_method(&resolver, "insert", "(Landroid/net/Uri;Landroid/content/ContentValues;)Landroid/net/Uri;", &[JValue::Object(&collection), JValue::Object(&values)])?
            .l()?;
        if uri.is_null() {
            return Err(jni::errors::Error::NullPtr("MediaStore.insert"));
        }
        let copy = |env: &mut JNIEnv, file: &mut std::fs::File| -> jni::errors::Result<()> {
            let os = env.call_method(&resolver, "openOutputStream", "(Landroid/net/Uri;)Ljava/io/OutputStream;", &[JValue::Object(&uri)])?.l()?;
            let mut buf = vec![0u8; 1 << 20];
            let result = (|| -> jni::errors::Result<()> {
                loop {
                    let n = std::io::Read::read(file, &mut buf).map_err(|_| jni::errors::Error::JavaException)?;
                    if n == 0 {
                        return Ok(());
                    }
                    let arr = env.byte_array_from_slice(&buf[..n])?;
                    env.call_method(&os, "write", "([BII)V", &[JValue::Object(&arr), JValue::Int(0), JValue::Int(n as i32)])?;
                    env.delete_local_ref(arr)?;
                }
            })();
            // Always close the stream, even after an error.
            let closed = env.call_method(&os, "close", "()V", &[]).map(|_| ());
            result.and(closed)
        };
        if let Err(e) = copy(env, &mut file) {
            // Remove the half-written entry from the gallery.
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            let _ = env.call_method(&resolver, "delete", "(Landroid/net/Uri;Ljava/lang/String;[Ljava/lang/String;)I", &[JValue::Object(&uri), JValue::Object(&JObject::null()), JValue::Object(&JObject::null())]);
            return Err(e);
        }
        // Publish.
        let done = env.new_object("android/content/ContentValues", "()V", &[])?;
        let zero = env.new_object("java/lang/Integer", "(I)V", &[JValue::Int(0)])?;
        env.call_method(&done, "put", "(Ljava/lang/String;Ljava/lang/Integer;)V", &[JValue::Object(&pending), JValue::Object(&zero)])?;
        env.call_method(&resolver, "update", "(Landroid/net/Uri;Landroid/content/ContentValues;Ljava/lang/String;[Ljava/lang/String;)I", &[JValue::Object(&uri), JValue::Object(&done), JValue::Object(&JObject::null()), JValue::Object(&JObject::null())])?;
        let s = env.call_method(&uri, "toString", "()Ljava/lang/String;", &[])?.l()?;
        let s: String = env.get_string(&jni::objects::JString::from(s))?.into();
        Ok(s)
    })
}

/// Open the Android share sheet for a content URI.
pub fn share(uri: &str, mime: &str) -> Result<(), String> {
    with_activity(|env, activity| {
        let uri_str = env.new_string(uri)?;
        let uri = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&uri_str)])?.l()?;
        let action = env.new_string("android.intent.action.SEND")?;
        let intent = env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&action)])?;
        let mime = env.new_string(mime)?;
        env.call_method(&intent, "setType", "(Ljava/lang/String;)Landroid/content/Intent;", &[JValue::Object(&mime)])?;
        let key = env.new_string("android.intent.extra.STREAM")?;
        env.call_method(&intent, "putExtra", "(Ljava/lang/String;Landroid/os/Parcelable;)Landroid/content/Intent;", &[JValue::Object(&key), JValue::Object(&uri)])?;
        env.call_method(&intent, "addFlags", "(I)Landroid/content/Intent;", &[JValue::Int(1)])?; // FLAG_GRANT_READ_URI_PERMISSION
        let title = env.new_string(crate::i18n::tr("Compartir animación"))?;
        let chooser = env
            .call_static_method("android/content/Intent", "createChooser", "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;", &[JValue::Object(&intent), JValue::Object(&title)])?
            .l()?;
        env.call_method(activity, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&chooser)])?;
        Ok(())
    })
}
