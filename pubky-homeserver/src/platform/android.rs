use std::{mem, path::PathBuf, ptr::NonNull};

use anyhow::{Context, Result};
use jni::{
    objects::{JObject, JString},
    JavaVM,
};

fn with_activity<F, R>(f: F) -> Result<R>
where
    F: FnOnce(&mut jni::JNIEnv<'_>, &JObject<'_>) -> Result<R>,
{
    let context = ndk_context::android_context();
    let vm_ptr = NonNull::new(context.vm().cast()).context("Android VM pointer is null")?;
    let activity_ptr =
        NonNull::new(context.context().cast()).context("Android activity pointer is null")?;

    let vm = unsafe { JavaVM::from_raw(vm_ptr.as_ptr()) }?;
    let mut env = vm.attach_current_thread()?;

    // SAFETY: The activity pointer is managed by the Android runtime and
    // remains valid for the entire process lifetime. We purposely avoid
    // releasing the reference when the JObject is dropped.
    let activity = unsafe { JObject::from_raw(activity_ptr.as_ptr()) };
    let result = f(&mut env, &activity);
    mem::forget(activity);
    result
}

fn dir_from_method(
    env: &mut jni::JNIEnv<'_>,
    activity: &JObject<'_>,
    method: &str,
) -> Result<PathBuf> {
    let dir = env
        .call_method(activity, method, "()Ljava/io/File;", &[])
        .with_context(|| format!("calling {method}()"))?
        .l()
        .context("expected java.io.File from method")?;
    let absolute_path: JString = env
        .call_method(dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
        .context("calling getAbsolutePath()")?
        .l()
        .context("expected java.lang.String from getAbsolutePath()")?
        .into();
    let path: String = env
        .get_string(&absolute_path)
        .context("converting Java string to Rust string")?
        .into();
    Ok(PathBuf::from(path))
}

/// Returns the Android application's internal files directory.
pub fn app_files_dir() -> Result<PathBuf> {
    with_activity(|env, activity| dir_from_method(env, activity, "getFilesDir"))
}

/// Returns the Android application's cache directory.
pub fn app_cache_dir() -> Result<PathBuf> {
    with_activity(|env, activity| dir_from_method(env, activity, "getCacheDir"))
}
