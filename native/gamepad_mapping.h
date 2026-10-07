#pragma once
#include <linux/input-event-codes.h>
// Action order: A, B, X, Y, LB, RB, View, Menu, LS, RS.
// Xbox/Android maps X to Linux 307 and Y to 308. The positional
// aliases WEST/NORTH do not match these letter names.
inline constexpr int keys[]={BTN_A,BTN_B,BTN_X,BTN_Y,BTN_TL,BTN_TR,BTN_SELECT,BTN_START,BTN_THUMBL,BTN_THUMBR};

inline constexpr int trigger_keys[]={BTN_TL2,BTN_TR2};
// Preserve analog travel while exposing Android's digital L2/R2 events.
// Press at 30/255 and release at 20/255 to avoid noisy edges.
struct GamepadTriggers {
 bool down[2]{};
 bool update(int side,int value){down[side]=down[side]?value>20:value>=30;return down[side];}
};
