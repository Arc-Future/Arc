/* RFC 037 §3 — Light default Theme tokens (Ant Design 6.x defaultAlgorithm).
 * Arc-side mirror: std/UI/Core/Themes/Light.arml (+ BuiltInTheme.as keys / geometry)
 * Authority: https://ant.design/docs/react/customize-theme
 * Honest: Seed/Map token alignment — not a pixel-level antd DOM/CSS clone. */

#ifndef ARC_RT_UI_DESIGN_TOKENS_H
#define ARC_RT_UI_DESIGN_TOKENS_H

#include <stdint.h>

/* §3.1 Color (0xAARRGGBB) — Ant Design 6 Light map tokens */
#define RT_UI_COLOR_BACKGROUND        0xFFF5F5F5u
#define RT_UI_COLOR_SURFACE           0xFFFFFFFFu
#define RT_UI_COLOR_BORDER            0xFFD9D9D9u
#define RT_UI_COLOR_BORDER_DISABLED   0xFFD9D9D9u /* colorBorderDisabled */
#define RT_UI_COLOR_TEXT_PRIMARY      0xE0000000u
#define RT_UI_COLOR_TEXT_SECONDARY    0xA6000000u
#define RT_UI_COLOR_PRIMARY           0xFF1677FFu
#define RT_UI_COLOR_PRIMARY_HOVER     0xFF4096FFu
#define RT_UI_COLOR_PRIMARY_PRESSED   0xFF0958D9u
#define RT_UI_COLOR_FOCUS_RING        0x661677FFu
#define RT_UI_COLOR_DISABLED_FILL     0xFFF5F5F5u
#define RT_UI_COLOR_DISABLED_TEXT     0xFFBFBFBFu
#define RT_UI_COLOR_TEXT_ON_PRIMARY   0xFFFFFFFFu
#define RT_UI_COLOR_SLIDER_TRACK      0xFFF0F0F0u
#define RT_UI_COLOR_TRANSPARENT       0x00000000u
#define RT_UI_COLOR_SURFACE_HOVER     0xFFE6F4FFu
#define RT_UI_COLOR_SCROLL_TRACK      0xFFF5F5F5u
#define RT_UI_COLOR_SCROLL_THUMB      0xFFBFBFBFu
#define RT_UI_COLOR_SCROLL_THUMB_HOVER 0xFF8C8C8Cu
#define RT_UI_COLOR_SCROLL_THUMB_PRESSED 0xFF595959u
#define RT_UI_COLOR_DANGER          0xFFFF4D4Fu /* colorError */
#define RT_UI_COLOR_DANGER_HOVER    0xFFFF7875u /* colorErrorHover */
#define RT_UI_COLOR_DANGER_PRESSED  0xFFD9363Eu /* colorErrorActive */

/* §3.2 Radius — Ant borderRadius / borderRadiusLG */
#define RT_UI_RADIUS_CONTROL          6
#define RT_UI_RADIUS_SURFACE          8

/* §3.3 Spacing — Ant sizeXX / margin rhythm (4px grid) */
#define RT_UI_SPACING_XS              4
#define RT_UI_SPACING_SM              8
#define RT_UI_SPACING_MD              12
#define RT_UI_SPACING_LG              16
#define RT_UI_SPACING_XL              24

/* §3.4 Typography — Ant fontSize / fontSizeSM */
#define RT_UI_FONT_BODY_SIZE          14.0
#define RT_UI_FONT_CAPTION_SIZE       12.0

/* §3.5 Elevation */
#define RT_UI_BORDER_HAIRLINE           1
#define RT_UI_FOCUS_RING_WIDTH          2

/* §6 control minimums — Ant controlHeight (+ padding 对齐 ControlMetrics) */
#define RT_UI_CONTROL_HEIGHT            32
#define RT_UI_BUTTON_MIN_HEIGHT         32
#define RT_UI_INPUT_MIN_HEIGHT          32
#define RT_UI_BUTTON_PADDING_X          16
#define RT_UI_BUTTON_PADDING_Y          8
#define RT_UI_TOGGLE_BOX_SIZE           16

#endif /* ARC_RT_UI_DESIGN_TOKENS_H */
