"""Compile externally supplied GPL reference methods with minimal UI stand-ins.

Only the harness and extractor live here. Reference method bodies remain in the
explicitly supplied private cache and must not be added to the repository.
"""
from pathlib import Path
import re,subprocess,sys
root=Path(sys.argv[1]).resolve()

def method(text, signature):
 start=text.index(signature);opening=text.index('{',start);depth=0;state=None;i=opening
 while i<len(text):
  c=text[i];n=text[i:i+2]
  if state=='line':
   if c=='\n':state=None
  elif state=='block':
   if n=='*/':state=None;i+=1
  elif state in ('"',"'"):
   if c=='\\':i+=1
   elif c==state:state=None
  elif n=='//':state='line';i+=1
  elif n=='/*':state='block';i+=1
  elif c in ('"',"'"):state=c
  elif c=='{':depth+=1
  elif c=='}':
   depth-=1
   if depth==0:return text[start:i+1]
  i+=1
 raise ValueError(signature)

prefix=r'''
#import <Foundation/Foundation.h>
#import <CoreGraphics/CoreGraphics.h>
#include <math.h>
#define UITouchPhaseEnded 3
#define UITouchPhaseCancelled 4
#define UITouchTypeIndirectPointer 3
int currentHotbarSlot=-1,referenceHotbar=-1;
int callback_SurfaceViewController_touchHotbar(CGFloat x,CGFloat y){return referenceHotbar;}
#define ACTION_DOWN 0
#define ACTION_UP 1
#define ACTION_MOVE 2
#define ACTION_MOVE_MOTION 3
#define GLFW_KEY_W 87
#define GLFW_KEY_A 65
#define GLFW_KEY_S 83
#define GLFW_KEY_D 68
#define GLFW_MOD_SHIFT 1
#define GLFW_MOUSE_BUTTON_LEFT 0
#define GLFW_MOUSE_BUTTON_RIGHT 1
#define GLFW_MOUSE_BUTTON_MIDDLE 2
#define SPECIALBTN_KEYBOARD -1
#define SPECIALBTN_TOGGLECTRL -2
#define SPECIALBTN_MOUSEPRI -3
#define SPECIALBTN_MOUSESEC -4
#define SPECIALBTN_VIRTUALMOUSE -5
#define SPECIALBTN_MOUSEMID -6
#define SPECIALBTN_SCROLLUP -7
#define SPECIALBTN_SCROLLDOWN -8
#define SPECIALBTN_MENU -9
#define DIRECTION_EAST 0
#define DIRECTION_NORTH_EAST 1
#define DIRECTION_NORTH 2
#define DIRECTION_NORTH_WEST 3
#define DIRECTION_SOUTH_WEST 5
#define DIRECTION_SOUTH_EAST 7
BOOL isControlModifiable=NO,isGrabbing=YES,leftShiftHeld=NO,virtualMouseEnabled=YES;
CGFloat lastXValue=0,lastYValue=0,resolutionScale=1;
CGRect virtualMouseFrame;CGPoint lastVirtualMousePoint;
NSMutableArray *events;
BOOL timed=NO;double callbackDelay=0;
void capture(NSArray *event){[events addObject:timed?@[@(callbackDelay),event]:event];}
#define JNI_TRUE YES
#define GLFW_KEY_F 70
#define GLFW_KEY_Q 81
#define UIGestureRecognizerStateBegan 1
#define UIGestureRecognizerStateChanged 2
#define UIGestureRecognizerStateEnded 3
#define UIGestureRecognizerStateRecognized 3
#define UIGestureRecognizerStateCancelled 4
#define UIGestureRecognizerStateFailed 5
#define dispatch_time oracle_dispatch_time
#define dispatch_get_main_queue oracle_dispatch_get_main_queue
#define dispatch_after oracle_dispatch_after
int64_t dispatch_time(int64_t now,int64_t delay){return delay;}
void *dispatch_get_main_queue(void){return NULL;}
void dispatch_after(int64_t delay,void*queue,void(^block)(void)){double old=callbackDelay;callbackDelay=delay/1e9;block();callbackDelay=old;}

CGFloat clamp(CGFloat v,CGFloat a,CGFloat b){return MIN(b,MAX(a,v));}
void CallbackBridge_nativeSendKey(int key,int scan,int down,int mods){capture(@[@"key",@(key),@(down),@(mods)]);}
void CallbackBridge_nativeSendMouseButton(int b,int down,int mods){capture(@[@"button",@(b),@(down),@(mods)]);}
void CallbackBridge_nativeSendCursorPos(int phase,CGFloat x,CGFloat y){[events addObject:@[@"cursor",@(phase),@(x),@(y)]];}
void CallbackBridge_nativeSendScroll(CGFloat x,CGFloat y){capture(@[@"scroll",@(x),@(y)]);}
void setPrefBool(NSString *key,BOOL value){}
@interface UIView:NSObject
@property CGRect frame;@property CGPoint center;@property BOOL hidden;@property CGFloat alpha;
@end
@implementation UIView @end
@interface Label:NSObject @property NSString *text;@end
@implementation Label @end
@interface UIEvent:NSObject @end
@implementation UIEvent @end
@interface UITouch:NSObject @property CGPoint point,previous;@property UIView *view;@property int phase,type;
-(CGPoint)locationInView:(id)v;-(CGPoint)previousLocationInView:(id)v;
@end
@implementation UITouch -(CGPoint)locationInView:(id)v{return self.point;}-(CGPoint)previousLocationInView:(id)v{return self.previous;}@end
@interface Field:UIView @property BOOL isFirstResponder;@property NSString *text;
-(void)resignFirstResponder;-(void)becomeFirstResponder;
@end
@implementation Field
-(void)resignFirstResponder{self.isFirstResponder=NO;if(timed)capture(@[@"keyboard"]);}-(void)becomeFirstResponder{self.isFirstResponder=YES;if(timed)capture(@[@"keyboard"]);}
@end
@interface ControlButton:UIView @property NSDictionary *properties;@property Label *titleLabel;@end
@implementation ControlButton @end
@interface Joystick:ControlButton @property UIView *thumb,*fwdLockView;@property CGPoint mCenter;
-(void)touchesMoved:(NSSet<UITouch*>*)t withEvent:(UIEvent*)e;
-(void)touchesEnded:(NSSet*)t withEvent:(UIEvent*)e;
-(void)callbackMoveX:(CGFloat)x Y:(CGFloat)y;
@end
'''
surface=r'''
@interface UIGestureRecognizer:NSObject @property int state;@property CGPoint location,velocity;
-(CGPoint)locationInView:(id)v;-(CGPoint)velocityInView:(id)v;
@end
@implementation UIGestureRecognizer
-(CGPoint)locationInView:(id)v{return self.location;}-(CGPoint)velocityInView:(id)v{return self.velocity;}
@end
#define UITapGestureRecognizer UIGestureRecognizer
#define UILongPressGestureRecognizer UIGestureRecognizer
#define UIPanGestureRecognizer UIGestureRecognizer
@interface Haptic:NSObject -(void)impactOccurred;@end
@implementation Haptic -(void)impactOccurred{}@end

@interface UIScreen:NSObject @property CGFloat scale;+(UIScreen*)mainScreen;@end
@implementation UIScreen +(UIScreen*)mainScreen{static UIScreen*s;if(!s)s=[UIScreen new];return s;}@end
@interface EventSink:NSObject
-(void)touchesBegan:(NSSet*)t withEvent:(UIEvent*)e;-(void)touchesMoved:(NSSet*)t withEvent:(UIEvent*)e;
-(void)touchesEnded:(NSSet*)t withEvent:(UIEvent*)e;-(void)touchesCancelled:(NSSet*)t withEvent:(UIEvent*)e;
@end
@implementation EventSink
-(void)touchesBegan:(NSSet*)t withEvent:(UIEvent*)e{}-(void)touchesMoved:(NSSet*)t withEvent:(UIEvent*)e{}
-(void)touchesEnded:(NSSet*)t withEvent:(UIEvent*)e{}-(void)touchesCancelled:(NSSet*)t withEvent:(UIEvent*)e{}
@end

@interface Surface:EventSink
@property UIView *rootView,*view;
@property Haptic *lightHaptic,*mediumHaptic;
@property BOOL shouldTriggerHaptic,enableMouseGestures;
-(void)surfaceOnClick:(UIGestureRecognizer*)g;-(void)surfaceOnLongpress:(UIGestureRecognizer*)g;
-(void)surfaceOnDoubleClick:(UIGestureRecognizer*)g;-(void)surfaceOnTouchesScroll:(UIGestureRecognizer*)g;
-(void)keyboardGesture:(UIGestureRecognizer*)g;
@property BOOL isMacCatalystApp,enableHotbarGestures,slideableHotbar,shouldTriggerClick;
@property CGRect clickRange;
@property UITouch *primaryTouch,*hotbarTouch;
-(BOOL)isTouchInactive:(UITouch*)touch;
-(void)sendTouchEvent:(UITouch*)touch withUIEvent:(UIEvent*)uievent withEvent:(int)event;
-(void)touchesEndedGlobal:(NSSet*)t withEvent:(UIEvent*)e;
 @property CGFloat screenScale,mouseSpeed;@property UIView *surfaceView,*mousePointerView;@property Field *inputTextField;
-(void)sendTouchPoint:(CGPoint)location withEvent:(int)event;
-(void)executebtn:(ControlButton*)sender withAction:(int)action;
-(void)executebtn_special_togglebtn:(int)held;-(void)actionOpenNavigationMenu;-(void)setNeedsUpdateOfPrefersPointerLocked;
@end
@implementation Surface
-(void)executebtn_special_togglebtn:(int)held{}-(void)actionOpenNavigationMenu{}-(void)setNeedsUpdateOfPrefersPointerLocked{}
'''
clock=r'''
#define MOUSE_MAX_ACCELERATION 2
CGFloat frameTimeValue=0,lastFrameTime=0;
CGFloat CACurrentMediaTime(void){return frameTimeValue;}
CGFloat MathUtils_dist(CGFloat a,CGFloat b,CGFloat c,CGFloat d){return hypot(c-a,d-b);}
#define SurfaceViewController Surface
@interface UIWindow:NSObject
@property id rootViewController;
+(UIWindow*)mainWindow;
@end
@implementation UIWindow
+(UIWindow*)mainWindow {static UIWindow *w; if(!w) w=[UIWindow new];return w;}
@end
@interface ControllerInput:NSObject +(void)tick; @end
@implementation ControllerInput
'''
main=r'''
int main(){@autoreleasepool{
 NSData *input=[[NSFileHandle fileHandleWithStandardInput]readDataToEndOfFile];NSDictionary *c=[NSJSONSerialization JSONObjectWithData:input options:0 error:nil];
 events=[NSMutableArray new];NSMutableArray *steps=[NSMutableArray new];
 CGFloat w=[c[@"width"]doubleValue],h=[c[@"height"]doubleValue];isGrabbing=[c[@"grabbed"]boolValue];leftShiftHeld=[c[@"shift"]boolValue];
 Joystick *j=[Joystick new];j.frame=CGRectMake(0,0,w,h);j.mCenter=CGPointMake(w/2,h/2);j.thumb=[UIView new];CGFloat knob=MIN(w,h)/4;j.thumb.frame=CGRectMake(0,0,knob,knob);j.thumb.center=j.mCenter;
 Surface *s=[Surface new];s.surfaceView=[UIView new];s.surfaceView.frame=CGRectMake(0,0,w,h);s.mousePointerView=[UIView new];s.mouseSpeed=c[@"speed"]?[c[@"speed"]doubleValue]:1;s.screenScale=c[@"scale"]?[c[@"scale"]doubleValue]:1;resolutionScale=c[@"resolution"]?[c[@"resolution"]doubleValue]:1;virtualMouseEnabled=c[@"virtual"]?[c[@"virtual"]boolValue]:YES;
 s.rootView=s.surfaceView;s.view=s.surfaceView;s.inputTextField=[Field new];UIScreen.mainScreen.scale=s.screenScale;NSMutableDictionary *contacts=[NSMutableDictionary new];
 UIWindow.mainWindow.rootViewController=s;
 virtualMouseFrame=CGRectMake(w/2,h/2,18,27);lastVirtualMousePoint=CGPointZero;
 for(NSDictionary *e in c[@"events"]){[events removeAllObjects];CGPoint p=CGPointMake([e[@"x"]doubleValue],[e[@"y"]doubleValue]);NSString *op=e[@"op"];
 if([c[@"mode"]isEqual:@"gestures"]){
 timed=YES;currentHotbarSlot=[e[@"slot"]intValue]<0?-1:49+[e[@"slot"]intValue];referenceHotbar=currentHotbarSlot;
 s.shouldTriggerClick=YES;s.enableMouseGestures=[c[@"mouse"]boolValue];s.enableHotbarGestures=YES;s.slideableHotbar=NO;
 UIGestureRecognizer*g=[UIGestureRecognizer new];g.state=[e[@"phase"]intValue];g.location=p;g.velocity=CGPointMake(p.x*w,p.y*h);
 switch([e[@"kind"]intValue]){case 1:[s surfaceOnClick:g];break;case 2:[s surfaceOnLongpress:g];break;case 3:[s surfaceOnDoubleClick:g];break;case 4:[s surfaceOnTouchesScroll:g];break;case 5:[s keyboardGesture:g];break;}
 }
 else if([c[@"mode"]isEqual:@"world_camera"] || [c[@"mode"]isEqual:@"menu_surface"]){
 NSNumber* tid=e[@"id"];UITouch *t=contacts[tid];if(!t){t=[UITouch new];contacts[tid]=t;}t.previous=t.point;t.point=p;t.view=s.rootView;
 t.phase=[op isEqual:@"up"]?UITouchPhaseEnded:[op isEqual:@"cancel"]?UITouchPhaseCancelled:0;
 NSSet*set=[NSSet setWithObject:t];
 if([op isEqual:@"down"])[s touchesBegan:set withEvent:nil];else if([op isEqual:@"up"])[s touchesEnded:set withEvent:nil];else if([op isEqual:@"cancel"])[s touchesCancelled:set withEvent:nil];else[s touchesMoved:set withEvent:nil];
 if([c[@"mode"]isEqual:@"menu_surface"]){NSMutableArray*normalized=[NSMutableArray new];for(NSArray*event in events){[normalized addObject:@[@"move",event[2],event[3]]];}events=normalized;}
 else {NSIndexSet *ignored=[events indexesOfObjectsPassingTest:^BOOL(NSArray*event,NSUInteger i,BOOL*stop){return [event[0]isEqual:@"cursor"] && [event[1]intValue]!=ACTION_MOVE_MOTION;}];[events removeObjectsAtIndexes:ignored];}
 }
 else if([c[@"mode"]isEqual:@"tick"]){lastXValue=[e[@"vx"]doubleValue];lastYValue=[e[@"vy"]doubleValue];frameTimeValue=[e[@"time"]doubleValue];[ControllerInput tick];}
 else if([c[@"mode"]isEqual:@"world_stick"]){UITouch *t=[UITouch new];t.point=p;t.view=j;NSSet *touches=[NSSet setWithObject:t];if([op isEqual:@"end"])[j touchesEnded:touches withEvent:nil];else[j touchesMoved:touches withEvent:nil];}
 else if([c[@"mode"]isEqual:@"world_buttons"] || [c[@"mode"]isEqual:@"menu_buttons"]){if(![op isEqual:@"move"]){ControlButton *b=[ControlButton new];b.properties=@{@"keycodes":@[e[@"code"],@0,@0,@0]};[s executebtn:b withAction:[op isEqual:@"down"]?ACTION_DOWN:ACTION_UP];}}
 else if([c[@"mode"]isEqual:@"stick"]){UITouch *t=[UITouch new];t.point=p;t.view=j;NSSet *touches=[NSSet setWithObject:t];if([op isEqual:@"end"])[j touchesEnded:touches withEvent:nil];else[j touchesMoved:touches withEvent:nil];}
 else if([c[@"mode"]isEqual:@"cursor"]){int action=[op isEqual:@"down"]?ACTION_DOWN:[op isEqual:@"up"]?ACTION_UP:[op isEqual:@"relative"]?ACTION_MOVE_MOTION:ACTION_MOVE;[s sendTouchPoint:p withEvent:action];}
 else{ControlButton *b=[ControlButton new];b.properties=@{@"keycodes":e[@"codes"]};[s executebtn:b withAction:[op isEqual:@"down"]?ACTION_DOWN:ACTION_UP];}
 if([c[@"mode"]isEqual:@"world_stick"])j.thumb.center=j.mCenter;
 [steps addObject:@{@"events":[events copy],@"velocity":@[@(lastXValue),@(lastYValue)],@"thumb":@[@(j.thumb.center.x),@(j.thumb.center.y)]}];
 }
 NSData *out=[NSJSONSerialization dataWithJSONObject:steps options:NSJSONWritingSortedKeys error:nil];fwrite(out.bytes,1,out.length,stdout);
}return 0;}
'''
for label,path in [('pojav',root),('amethyst',root/'amethyst')]:
 j=(path/'Natives/customcontrols/ControlJoystick.m').read_text();s=(path/'Natives/SurfaceViewController.m').read_text()
 controller=(path/('Natives/input/ControllerInput.m' if label=='pojav' else 'Natives/ControllerInput.m')).read_text()
 code=prefix+'\n@implementation Joystick\n'+'\n'.join(method(j,x) for x in ['- (void)touchesMoved:', '- (void)touchesEnded:', '- (void)callbackMoveX:'])+'\n@end\n'+surface+'\n'.join(method(s,x)for x in ['- (void)sendTouchPoint:', '- (void)executebtn:', '- (BOOL)isTouchInactive:', '- (void)sendTouchEvent:', '- (void)touchesBegan:', '- (void)touchesMoved:', '- (void)touchesEndedGlobal:', '- (void)touchesEnded:', '- (void)touchesCancelled:', '- (void)surfaceOnClick:', '-(void)surfaceOnLongpress:', '- (void)surfaceOnDoubleClick:', '- (void)surfaceOnTouchesScroll:', '- (void)keyboardGesture:'])+'\n@end\n'+clock+method(controller,'+ (void)tick')+'\n@end\n'+main
 file=root/(label+'-oracle.m');file.write_text('// Generated development oracle with GPLv3 reference method bodies; not for distribution.\n'+code)
 p=subprocess.run(['clang','-fobjc-arc','-Wno-objc-property-no-attribute','-Wno-incompatible-pointer-types','-framework','Foundation','-framework','CoreGraphics',str(file),'-o',str(root/(label+'-oracle'))],capture_output=True,text=True)
 (root/(label+'-compile.log')).write_text(p.stdout+p.stderr);print(label,p.returncode,p.stderr[:2500]);assert p.returncode==0
