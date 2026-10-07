#pragma once
#include <linux/input-event-codes.h>
// Action order: A, B, X, Y, LB, RB, View, Menu, LS, RS.
// Xbox/Android maps X to Linux 307 and Y to 308. The positional
// aliases WEST/NORTH do not match these letter names.
inline constexpr int keys[]={BTN_A,BTN_B,BTN_X,BTN_Y,BTN_TL,BTN_TR,BTN_SELECT,BTN_START,BTN_THUMBL,BTN_THUMBR};

inline constexpr int trigger_keys[]={BTN_TL2,BTN_TR2};
// Preserve analog travel while exposing Android's digital L2/R2 events.
// Default press at 80%; release five percentage points below it to avoid noise.
struct GamepadTriggers {
 bool down[2]{};
 int press=204,release=191;
 bool configure(int percent){if(percent<1||percent>100)return false;press=(percent*255+99)/100;release=(percent>5?percent-5:0)*255/100;return true;}
 bool update(int side,int value){down[side]=down[side]?value>release:value>=press;return down[side];}
};
