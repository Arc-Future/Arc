/* rt_sqlite.c — L3 Orm SQLite execute MVP（prepare/step 最小绿路径）
 *
 * 包装 vendored sqlite amalgamation（crates/runtime-sqlite/sqlite3.{c,h}）。
 * 句柄为 1-based slot id（0 = 无效）；不扩 IDbProvider 协议。
 */

#include "rt_abi.h"

#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#include "sqlite3.h"

#define RT_SQLITE_MAX_DB   32
#define RT_SQLITE_MAX_STMT 64

static sqlite3* g_dbs[RT_SQLITE_MAX_DB];
static sqlite3_stmt* g_stmts[RT_SQLITE_MAX_STMT];
static int32_t g_stmt_db[RT_SQLITE_MAX_STMT]; /* owning db slot (1-based), 0 unused */

static sqlite3* db_from(int32_t h) {
    if (h <= 0 || h > RT_SQLITE_MAX_DB) return NULL;
    return g_dbs[h - 1];
}

static sqlite3_stmt* stmt_from(int32_t h) {
    if (h <= 0 || h > RT_SQLITE_MAX_STMT) return NULL;
    return g_stmts[h - 1];
}

static char* rt_sqlite_strdup(const char* s) {
    if (!s) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    size_t n = strlen(s);
    char* out = (char*)malloc(n + 1);
    if (!out) return NULL;
    memcpy(out, s, n + 1);
    return out;
}

int32_t rt_sqlite_open(const char* path) {
    const char* p = (path && path[0]) ? path : ":memory:";
    int32_t i;
    for (i = 0; i < RT_SQLITE_MAX_DB; i++) {
        if (g_dbs[i] == NULL) {
            sqlite3* db = NULL;
            if (sqlite3_open(p, &db) != SQLITE_OK) {
                if (db) sqlite3_close(db);
                return 0;
            }
            g_dbs[i] = db;
            return i + 1;
        }
    }
    return 0;
}

void rt_sqlite_close(int32_t db_handle) {
    sqlite3* db = db_from(db_handle);
    if (!db) return;
    /* finalize stmts owned by this db */
    int32_t i;
    for (i = 0; i < RT_SQLITE_MAX_STMT; i++) {
        if (g_stmt_db[i] == db_handle && g_stmts[i]) {
            sqlite3_finalize(g_stmts[i]);
            g_stmts[i] = NULL;
            g_stmt_db[i] = 0;
        }
    }
    sqlite3_close(db);
    g_dbs[db_handle - 1] = NULL;
}

int32_t rt_sqlite_exec(int32_t db_handle, const char* sql) {
    sqlite3* db = db_from(db_handle);
    if (!db || !sql) return -1;
    char* errmsg = NULL;
    int rc = sqlite3_exec(db, sql, NULL, NULL, &errmsg);
    if (errmsg) sqlite3_free(errmsg);
    if (rc != SQLITE_OK) return -1;
    return (int32_t)sqlite3_changes(db);
}

int32_t rt_sqlite_prepare(int32_t db_handle, const char* sql) {
    sqlite3* db = db_from(db_handle);
    if (!db || !sql) return 0;
    int32_t i;
    for (i = 0; i < RT_SQLITE_MAX_STMT; i++) {
        if (g_stmts[i] == NULL) {
            sqlite3_stmt* stmt = NULL;
            if (sqlite3_prepare_v2(db, sql, -1, &stmt, NULL) != SQLITE_OK) {
                return 0;
            }
            g_stmts[i] = stmt;
            g_stmt_db[i] = db_handle;
            return i + 1;
        }
    }
    return 0;
}

/* 100 = ROW, 101 = DONE, 其它 = 错误（对齐 sqlite3 常量，便于 Arc 侧对照） */
int32_t rt_sqlite_step(int32_t stmt_handle) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt) return -1;
    return (int32_t)sqlite3_step(stmt);
}

int32_t rt_sqlite_column_count(int32_t stmt_handle) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt) return 0;
    return (int32_t)sqlite3_column_count(stmt);
}

/* 1=INTEGER 2=FLOAT 3=TEXT 4=BLOB 5=NULL（对齐 sqlite3_column_type 常量） */
int32_t rt_sqlite_column_type(int32_t stmt_handle, int32_t col) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt || col < 0) return 0;
    return (int32_t)sqlite3_column_type(stmt, col);
}

int32_t rt_sqlite_column_int(int32_t stmt_handle, int32_t col) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt || col < 0) return 0;
    return (int32_t)sqlite3_column_int(stmt, col);
}

double rt_sqlite_column_double(int32_t stmt_handle, int32_t col) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt || col < 0) return 0.0;
    return sqlite3_column_double(stmt, col);
}

char* rt_sqlite_column_text(int32_t stmt_handle, int32_t col) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt || col < 0) return rt_sqlite_strdup("");
    const unsigned char* t = sqlite3_column_text(stmt, col);
    return rt_sqlite_strdup(t ? (const char*)t : "");
}

char* rt_sqlite_column_name(int32_t stmt_handle, int32_t col) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt || col < 0) return rt_sqlite_strdup("");
    const char* n = sqlite3_column_name(stmt, col);
    return rt_sqlite_strdup(n ? n : "");
}

void rt_sqlite_finalize(int32_t stmt_handle) {
    if (stmt_handle <= 0 || stmt_handle > RT_SQLITE_MAX_STMT) return;
    int32_t i = stmt_handle - 1;
    if (g_stmts[i]) {
        sqlite3_finalize(g_stmts[i]);
        g_stmts[i] = NULL;
        g_stmt_db[i] = 0;
    }
}

char* rt_sqlite_errmsg(int32_t db_handle) {
    sqlite3* db = db_from(db_handle);
    if (!db) return rt_sqlite_strdup("invalid db handle");
    return rt_sqlite_strdup(sqlite3_errmsg(db));
}

/* sqlite bind indices are 1-based; return 0 ok, -1 fail */
int32_t rt_sqlite_bind_text(int32_t stmt_handle, int32_t idx, const char* text) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt || idx < 1) return -1;
    const char* t = text ? text : "";
    if (sqlite3_bind_text(stmt, idx, t, -1, SQLITE_TRANSIENT) != SQLITE_OK) return -1;
    return 0;
}

int32_t rt_sqlite_bind_int(int32_t stmt_handle, int32_t idx, int32_t value) {
    sqlite3_stmt* stmt = stmt_from(stmt_handle);
    if (!stmt || idx < 1) return -1;
    if (sqlite3_bind_int(stmt, idx, value) != SQLITE_OK) return -1;
    return 0;
}

int32_t rt_sqlite_changes(int32_t db_handle) {
    sqlite3* db = db_from(db_handle);
    if (!db) return -1;
    return (int32_t)sqlite3_changes(db);
}
