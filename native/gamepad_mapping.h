#pragma once
#include <linux/input-event-codes.h>
// Action order: A, B, X, Y, LB, RB, View, Menu, LS, RS.
// Xbox/Android maps X to Linux 307 and Y to 308. The positional
// aliases WEST/NORTH do not match these letter names.
inline constexpr int keys[]={BTN_A,BTN_B,BTN_X,BTN_Y,BTN_TL,BTN_TR,BTN_SELECT,BTN_START,BTN_THUMBL,BTN_THUMBR};
