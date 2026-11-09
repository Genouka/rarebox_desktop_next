package com.rarebox.desktop;

import dev.gpui.mobile.GpuiInputActivity;

/**
 * NativeActivity host for the Rust `rarebox_desktop` cdylib.
 *
 * GpuiInputActivity loads the library named by the `android.app.lib_name`
 * manifest meta-data and provides the IME bridge used by GPUI text fields.
 */
public final class MainActivity extends GpuiInputActivity {}
