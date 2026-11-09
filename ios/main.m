// RareBox Desktop — iOS host.
//
// UIKit owns the run loop; GPUI renders into the window created by
// `gpui_ios_run_demo()`. A CADisplayLink drives frames only while GPUI asks
// for them, so an idle screen costs no CPU.

#import <UIKit/UIKit.h>
#import <QuartzCore/QuartzCore.h>

#import "gpui_ios.h"

@interface RareBoxAppDelegate : UIResponder <UIApplicationDelegate>
@property (nonatomic, assign) void *gpuiApp;
@property (nonatomic, assign) void *gpuiWindow;
@property (nonatomic, strong) CADisplayLink *displayLink;
@end

static void RareBoxResumeFrames(void *context);

@implementation RareBoxAppDelegate

- (BOOL)application:(UIApplication *)application
    didFinishLaunchingWithOptions:(NSDictionary *)launchOptions {
    self.gpuiApp = gpui_ios_initialize();
    gpui_ios_register_app();
    gpui_ios_run_demo();

    self.gpuiWindow = gpui_ios_get_window();
    if (self.gpuiWindow) {
        self.displayLink = [CADisplayLink displayLinkWithTarget:self
                                                       selector:@selector(renderFrame)];
        [self.displayLink addToRunLoop:[NSRunLoop mainRunLoop]
                               forMode:NSRunLoopCommonModes];
        gpui_ios_set_frame_waker(self.gpuiWindow, RareBoxResumeFrames,
                                 (__bridge void *)self);
    }
    return YES;
}

- (void)renderFrame {
    if (self.gpuiWindow && !gpui_ios_request_frame(self.gpuiWindow)) {
        self.displayLink.paused = YES;
    }
}

static void RareBoxResumeFrames(void *context) {
    RareBoxAppDelegate *delegate = (__bridge RareBoxAppDelegate *)context;
    delegate.displayLink.paused = NO;
}

- (void)applicationWillEnterForeground:(UIApplication *)application {
    gpui_ios_will_enter_foreground(self.gpuiApp);
    if (!self.displayLink && self.gpuiWindow) {
        self.displayLink = [CADisplayLink displayLinkWithTarget:self
                                                       selector:@selector(renderFrame)];
        [self.displayLink addToRunLoop:[NSRunLoop mainRunLoop]
                               forMode:NSRunLoopCommonModes];
        gpui_ios_set_frame_waker(self.gpuiWindow, RareBoxResumeFrames,
                                 (__bridge void *)self);
    }
}

- (void)applicationDidBecomeActive:(UIApplication *)application {
    gpui_ios_did_become_active(self.gpuiApp);
}

- (void)applicationWillResignActive:(UIApplication *)application {
    gpui_ios_will_resign_active(self.gpuiApp);
}

- (void)applicationDidEnterBackground:(UIApplication *)application {
    gpui_ios_did_enter_background(self.gpuiApp);
    [self.displayLink invalidate];
    self.displayLink = nil;
}

- (BOOL)application:(UIApplication *)application
              openURL:(NSURL *)url
              options:(NSDictionary<UIApplicationOpenURLOptionsKey, id> *)options {
    gpui_ios_handle_open_url((__bridge void *)[url absoluteString]);
    return YES;
}

- (void)applicationWillTerminate:(UIApplication *)application {
    [self.displayLink invalidate];
    self.displayLink = nil;
    gpui_ios_will_terminate(self.gpuiApp);
}

@end

int main(int argc, char *argv[]) {
    @autoreleasepool {
        return UIApplicationMain(argc, argv, nil,
                                 NSStringFromClass([RareBoxAppDelegate class]));
    }
}
