#ifndef ARC_RT_UI_DESIGN_TOKENS_H
#define ARC_RT_UI_DESIGN_TOKENS_H

/* RFC 037 §3 — Light default Theme tokens (software raster canonical).
 * Arc-side mirror: std/UI/Core/Themes/Light.arml (+ BuiltInTheme.as keys / geometry) */

#include <stdint.h>

/* §3.1 Color (0xAARRGGBB) */
#define RT_UI_COLOR_BACKGROUND        0xFFFAFAFAu
#define RT_UI_COLOR_SURFACE           0xFFFFFFFFu
#define RT_UI_COLOR_BORDER            0xFFE6E6ECu
#define RT_UI_COLOR_TEXT_PRIMARY      0xFF1A1A1Au
#define RT_UI_COLOR_TEXT_SECONDARY    0xFF8A8A93u
#define RT_UI_COLOR_PRIMARY           0xFF4F46E5u
#define RT_UI_COLOR_PRIMARY_HOVER     0xFF6366F1u
#define RT_UI_COLOR_PRIMARY_PRESSED   0xFF4338CAu
#define RT_UI_COLOR_FOCUS_RING        0x734F46E5u
#define RT_UI_COLOR_DISABLED_FILL     0xFFF3F3F6u
#define RT_UI_COLOR_DISABLED_TEXT     0xFFB8B8C0u
#define RT_UI_COLOR_TEXT_ON_PRIMARY   0xFFFFFFFFu
#define RT_UI_COLOR_SLIDER_TRACK      0xFFE0E0E6u
#define RT_UI_COLOR_TRANSPARENT       0x00000000u
#define RT_UI_COLOR_SURFACE_HOVER     0xFFF4F4FFu
#define RT_UI_COLOR_SCROLL_TRACK      0xFFF0F0F0u
#define RT_UI_COLOR_SCROLL_THUMB      0xFF8A8A93u
#define RT_UI_COLOR_SCROLL_THUMB_HOVER 0xFF6E6E7Au
#define RT_UI_COLOR_SCROLL_THUMB_ACTIVE 0xFF4A4A55u

/* §3.2 Radius */
#define RT_UI_RADIUS_CONTROL          6
#define RT_UI_RADIUS_SURFACE          8

/* §3.3 Spacing */
#define RT_UI_SPACING_XS              4
#define RT_UI_SPACING_SM              8
#define RT_UI_SPACING_MD              12
#define RT_UI_SPACING_LG              16
#define RT_UI_SPACING_XL              24

/* §3.4 Typography */
#define RT_UI_FONT_BODY_SIZE          14.0
#define RT_UI_FONT_CAPTION_SIZE       12.0

/* §3.5 Elevation */
#define RT_UI_BORDER_HAIRLINE           1
#define RT_UI_FOCUS_RING_WIDTH          2

/* §6 control minimums */
#define RT_UI_BUTTON_MIN_HEIGHT         32
#define RT_UI_INPUT_MIN_HEIGHT          32

#endif /* ARC_RT_UI_DESIGN_TOKENS_H */
