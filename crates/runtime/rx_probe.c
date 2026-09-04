#include "rt_regex.c"
#include <stdio.h>

typedef struct { int32_t length; int32_t elem_size; } RTH;
void* rt_array_create(int32_t cap, int32_t elem_size){RTH* h=malloc(sizeof(RTH)+(size_t)cap*elem_size);h->length=cap;h->elem_size=elem_size;memset((char*)h+sizeof(RTH),0,(size_t)cap*elem_size);return (char*)h+sizeof(RTH);}
int32_t rt_array_length(void* p){return ((RTH*)((char*)p-sizeof(RTH)))->length;}

static void g(const char* pat, const char* in, int grp){
    char* m=rt_regex_match_group_opt(pat,in,grp,0);
    printf("group(%d) %-22s | %-12s -> [%s]\n",grp,pat,in,m?m:"(null)"); free(m);
}
static void i(const char* pat,const char* in){printf("is %-22s | %-12s -> %d\n",pat,in,rt_regex_is_match_opt(pat,in,0));}
static void m(const char* pat,const char* in){char* r=rt_regex_match_opt(pat,in,0);printf("match %-22s | %-12s -> [%s]\n",pat,in,r?r:"(null)");free(r);}
static void rp(const char* pat,const char* in,const char* rep){char* r=rt_regex_replace_opt(pat,in,rep,0);printf("repl  %-22s | %-12s rep=%s -> [%s]\n",pat,in,rep,r?r:"(null)");free(r);}
static void q(const char* pat,const char* in){void* a=rt_regex_matches_opt(pat,in,0);int n=rt_array_length(a);char** it=(char**)a;printf("matchs%-22s | %-12s len=%d ->",pat,in,n);for(int k=0;k<n;k++)printf(" [%s]",it[k]?it[k]:"(null)");printf("\n");}

int main(void){
    setvbuf(stdout,NULL,_IONBF,0);
    printf("=== replacement group caps ===\n");
    g("(\\d)(\\d)","12",1); g("(\\d)(\\d)","12",2);
    g("(\\w+)@(\\w+)","user@host",1); g("(\\w+)@(\\w+)","user@host",2);
    rp("(\\w+)@(\\w+)","user@host","$2.$1");
    rp("(\\d)(\\d)","12","$2$1");
    rp("\\d","a1b2c","-");
    printf("=== verified declaration order ===\n");
    g("((a)(b))","ab",1);g("((a)(b))","ab",2);g("((a)(b))","ab",3);
    printf("=== quantifier bounds ===\n");
    i("a{2}","aa"); i("a{2}","a"); i("a{2,3}","a"); i("a{,2}","aa"); i("a{,2}","aaa"); i("a{,2}","aaaa");
    printf("=== backref named/numeric ===\n");
    i("(\\w+)\\s\\1","hello world"); i("(?<word>hi)-\\k<word>","hi-bye");
    i("(\\w+)\\s\\1","hello hello"); i("(?<word>hi)-\\k<word>","hi-hi");
    g("(\\w+)\\s\\1","hello world",1);
    printf("=== lookbehind ===\n");
    m("(?<=ab)c","abc"); i("(?<=ab)c","abc"); i("(?<=ab)c","ac");
    m("(?<!ab)c","xbc");
    printf("=== done ===\n");
    return 0;
}