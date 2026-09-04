#ifndef ARC_RT_UI_SYNTAX_TOKENS_H
#define ARC_RT_UI_SYNTAX_TOKENS_H

/* RFC 037 DrawText / RFC 037 Theme — CodeEditor syntax token colors (Light).
 * Arc-side mirror: std/UI/Core/Styling/DesignTokens.as (editor extensions). */

#include "rt_ui_design_tokens.h"
#include <stdint.h>
#include <string.h>

/* Editor chrome */
#define RT_UI_SYNTAX_EDITOR_BG          RT_UI_COLOR_SURFACE
#define RT_UI_SYNTAX_LINE_NUMBER        RT_UI_COLOR_TEXT_SECONDARY

/* Token kinds — align RFC 037 primary/secondary + common editor hues */
#define RT_UI_SYNTAX_DEFAULT            RT_UI_COLOR_TEXT_PRIMARY
#define RT_UI_SYNTAX_KEYWORD            RT_UI_COLOR_PRIMARY
#define RT_UI_SYNTAX_STRING             0xFF389E0Du  /* green */
#define RT_UI_SYNTAX_COMMENT            RT_UI_COLOR_TEXT_SECONDARY
#define RT_UI_SYNTAX_NUMBER             0xFFD46B08u  /* orange */
#define RT_UI_SYNTAX_TYPE               0xFF722ED1u  /* purple */
#define RT_UI_SYNTAX_OPERATOR           RT_UI_COLOR_TEXT_PRIMARY
#define RT_UI_SYNTAX_IDENTIFIER         RT_UI_COLOR_TEXT_PRIMARY

static inline uint32_t rt_ui_syntax_token_color(const char* kind) {
    if (!kind || !kind[0]) return RT_UI_SYNTAX_DEFAULT;
    if (strcmp(kind, "Keyword") == 0) return RT_UI_SYNTAX_KEYWORD;
    if (strcmp(kind, "String") == 0) return RT_UI_SYNTAX_STRING;
    if (strcmp(kind, "Comment") == 0) return RT_UI_SYNTAX_COMMENT;
    if (strcmp(kind, "Number") == 0) return RT_UI_SYNTAX_NUMBER;
    if (strcmp(kind, "Type") == 0) return RT_UI_SYNTAX_TYPE;
    if (strcmp(kind, "Operator") == 0) return RT_UI_SYNTAX_OPERATOR;
    if (strcmp(kind, "Identifier") == 0) return RT_UI_SYNTAX_IDENTIFIER;
    return RT_UI_SYNTAX_DEFAULT;
}

#endif /* ARC_RT_UI_SYNTAX_TOKENS_H */
