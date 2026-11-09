//
//  gpui_ios.h
//  C ABI exposed by the Rust static library for the iOS host.
//

#ifndef GPUI_IOS_H
#define GPUI_IOS_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

void* gpui_ios_initialize(void);
void  gpui_ios_did_finish_launching(void* app_ptr);
void  gpui_ios_will_enter_foreground(void* app_ptr);
void  gpui_ios_did_become_active(void* app_ptr);
void  gpui_ios_will_resign_active(void* app_ptr);
void  gpui_ios_did_enter_background(void* app_ptr);
void  gpui_ios_will_terminate(void* app_ptr);
void  gpui_ios_handle_touch(void* window_ptr, void* touch_ptr, void* event_ptr);
bool  gpui_ios_request_frame(void* window_ptr);
void  gpui_ios_set_frame_waker(void* window_ptr, void (*waker)(void* context), void* context);
void* gpui_ios_get_window(void);

// Defined by the application crate (this project), not by gpui-mobile.
void  gpui_ios_register_app(void);
void  gpui_ios_run_demo(void);

void  gpui_ios_show_keyboard(void* window_ptr);
void  gpui_ios_hide_keyboard(void* window_ptr);
void  gpui_ios_handle_text_input(void* window_ptr, void* text_ptr);
void  gpui_ios_handle_key_event(void* window_ptr, uint32_t key_code, uint32_t modifiers, _Bool is_key_down);
void  gpui_ios_handle_open_url(void* url_ptr);

#ifdef __cplusplus
}
#endif

#endif /* GPUI_IOS_H */
