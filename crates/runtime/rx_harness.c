#include "rt_regex.c"
#include <stdio.h>

typedef struct { int32_t length; int32_t elem_size; } RtArrayHeader;
void* rt_array_create(int32_t cap, int32_t elem_size) {
    size_t bytes = sizeof(RtArrayHeader) + (size_t)cap * (size_t)elem_size;
    RtArrayHeader* h = (RtArrayHeader*)malloc(bytes);
    if (!h) return NULL;
    h->length = cap; h->elem_size = elem_size;
    memset((char*)h + sizeof(RtArrayHeader), 0, (size_t)cap * (size_t)elem_size);
    return (char*)h + sizeof(RtArrayHeader);
}
int32_t rt_array_length(void* payload) {
    return ((RtArrayHeader*)((char*)payload - sizeof(RtArrayHeader)))->length;
}

static void t_is(const char* pat, const char* in, int opt) {
    int r = rt_regex_is_match_opt(pat, in, opt);
    printf("is_match  %-28s | %-16s | opt=%d -> %d\n", pat, in, opt, r);
}

static void t_match(const char* pat, const char* in, int opt) {
    char* m = rt_regex_match_opt(pat, in, opt);
    printf("match     %-28s | %-16s -> [%s]\n", pat, in, m ? m : "(null)");
    free(m);
}

static void t_match_group(const char* pat, const char* in, int grp, int opt) {
    char* m = rt_regex_match_group_opt(pat, in, grp, opt);
    printf("group(%d)  %-28s | %-16s -> [%s]\n", grp, pat, in, m ? m : "(null)");
    free(m);
}

static void t_replace(const char* pat, const char* in, const char* repl, int opt) {
    char* m = rt_regex_replace_opt(pat, in, repl, opt);
    printf("replace   %-28s | %-16s -> [%s]\n", pat, in, m ? m : "(null)");
    free(m);
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("=== IsMatch/Lookup core ===\n");
    t_is("hello", "say hello world", 0);
    t_is("h.llo", "say hello world", 0);
    t_is("xyz", "hello", 0);
    t_is("^abc", "abcdef", 0);
    t_is("def$", "abcdef", 0);
    t_is("[0-9]+", "abc123xyz", 0);
    t_is("[^0-9]+", "abc123", 0);
    t_is("\\d+", "plate 42", 0);
    t_is("\\w+", "  hello!", 0);
    t_is("\\s+", "a b", 0);
    t_is("(cat|dog)", "I have a dog", 0);
    t_is("colou?r", "color", 0);
    t_is("a+", "caaats", 0);
    t_is("ab*c", "abbbc", 0);
    t_is("a{2,3}", "aaa", 0);
    t_is("a{2,3}", "a", 0);
    t_is("a{2}", "aa", 0);
    t_is("a{2,}", "aaaaa", 0);
    t_is("a{,2}", "aa", 0);

    printf("=== Lazy / Named / Backref ===\n");
    t_match("a+?", "aaaa", 0);
    t_match("a{2,4}?", "aaaa", 0);
    t_match("a.*?b", "aXbYb", 0);
    t_match(".*?", "hello", 0);
    t_is("(?<word>hi)-\\k<word>", "hi-hi", 0);
    t_is("(\\w+)\\s\\1", "hello hello", 0);
    t_is("(\\w+)\\s\\1", "hello world", 0);

    printf("=== Lookaround ===\n");
    t_is("ab(?=c)", "abc", 0);
    t_is("ab(?!c)", "abc", 0);
    t_match("(?<=ab)c", "abc", 0);
    t_is("(?<!ab)c", "abc", 0);

    printf("=== Atomic ===\n");
    t_is("(a*)a", "aaa", 0);
    t_is("(?>a*)a", "aaa", 0);

    printf("=== Flags ===\n");
    t_is("(?i)hello", "HELLO", 0);
    t_is("(?m)^world", "hello\nworld", 0);
    t_is("(?s)a.b", "a\nb", 0);
    t_is("(?i:ab)c", "ABc", 0);
    t_is("(?i:ab)c", "ABC", 0);

    printf("=== WordAnchors ===\n");
    t_is("\\bworld\\b", "hello world", 0);
    t_is("\\Aabc\\z", "abc", 0);
    t_is("\\Aabc\\z", "abcd", 0);

    printf("=== Options ===\n");
    t_is("hello", "HELLO", 1);
    t_is("é", "É", 1);
    t_is("^world", "hello\nworld", 2);
    t_is("a.b", "a\nb", 4);

    printf("=== Negatives (should be all 0) ===\n");
    t_is("\\d+", "plate", 0);
    t_is("[0-9]+", "abcxyz", 0);
    t_is("a{2,3}", "a", 0);
    t_is("ab(?=c)", "abd", 0);
    t_is("\\bworld\\b", "helloworld", 0);

    printf("=== Matches / Split (array paths) ===\n");
    {
        void* a = rt_regex_matches_opt("\\d+", "1a22b333c", 0);
        int n = rt_array_length(a);
        char** it = (char**)a;
        printf("matches \\d+ (1a22b333c) len=%d ->", n);
        for (int i = 0; i < n; i++) printf(" [%s]", it[i]);
        printf("\n");
        void* z = rt_regex_matches_opt("\\d+", "abc", 0);
        printf("matches \\d+ (abc) len=%d\n", rt_array_length(z));
        void* io = rt_regex_matches_opt("\\d+", "1a2b", 1);
        printf("matches \\d+ opt=1 len=%d\n", rt_array_length(io));
        void* s = rt_regex_split_opt(",", "a,b,c", 0);
        int sn = rt_array_length(s);
        char** si = (char**)s;
        printf("split ',' (a,b,c) len=%d ->", sn);
        for (int i = 0; i < sn; i++) printf(" [%s]", si[i]);
        printf("\n");
    }
    printf("=== MatchGroup / Replace ===\n");
    t_match_group("(?<year>\\d{4})", "born 2026 ok", 1, 0);
    t_match_group("key=(\\w+)", "key=alpha", 0, 0);
    t_replace("(\\w+)@(\\w+)", "user@host", "$2.$1", 0);
    t_replace("cat", "Cat CAT", "dog", 1);
    t_replace("(\\d)(\\d)", "12", "$2$1", 0);

    printf("=== <b>hi</b> greedy ===\n");
    t_match("<.+>", "<b>hi</b>", 0);
    printf("=== done ===\n");
    return 0;
}