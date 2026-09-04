//! FileStream runtime ABI（标准库就绪 P0）。
//!
//! 符号前缀：`rt_file_stream_*`。对标 C# System.IO.FileStream 最小同步面。
//! 句柄为堆上 `RtFileStream`；失败 open 返回 NULL（Arc 侧须校验）。

#include "rt_abi.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#ifdef _WIN32
#include <io.h>
#include <fcntl.h>
#else
#include <unistd.h>
#include <sys/types.h>
#endif

typedef struct RtFileStream {
    FILE* fp;
    int32_t mode; /* 0=read, 1=write, 2=create */
    int32_t closed;
} RtFileStream;

static RtFileStream* rt_file_stream_from(void* handle) {
    return (RtFileStream*)handle;
}

/// mode: 0=rb, 1=wb, 2=wb（Create 与 OpenWrite 同为截断写）。
void* rt_file_stream_open(const char* path, int32_t mode) {
    if (!path) return NULL;
    const char* fmode = "rb";
    if (mode == 1 || mode == 2) fmode = "wb";
    FILE* fp = fopen(path, fmode);
    if (!fp) return NULL;
    RtFileStream* s = (RtFileStream*)malloc(sizeof(RtFileStream));
    if (!s) {
        fclose(fp);
        return NULL;
    }
    s->fp = fp;
    s->mode = mode;
    s->closed = 0;
    return s;
}

void rt_file_stream_close(void* handle) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed) return;
    if (s->fp) {
        fclose(s->fp);
        s->fp = NULL;
    }
    s->closed = 1;
    free(s);
}

int32_t rt_file_stream_read(void* handle, void* buffer, int32_t offset, int32_t count) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp || !buffer || offset < 0 || count <= 0) return 0;
    /* buffer 为 Arc 数组 payload（元素为 byte） */
    unsigned char* bytes = (unsigned char*)buffer + (size_t)offset;
    size_t n = fread(bytes, 1, (size_t)count, s->fp);
    return (int32_t)n;
}

void rt_file_stream_write(void* handle, void* buffer, int32_t offset, int32_t count) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp || !buffer || offset < 0 || count <= 0) return;
    unsigned char* bytes = (unsigned char*)buffer + (size_t)offset;
    fwrite(bytes, 1, (size_t)count, s->fp);
}

int64_t rt_file_stream_seek(void* handle, int64_t offset, int32_t origin) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp) return -1;
    int whence = SEEK_SET;
    if (origin == 1) whence = SEEK_CUR;
    else if (origin == 2) whence = SEEK_END;
    if (fseek(s->fp, (long)offset, whence) != 0) return -1;
#ifdef _WIN32
    return (int64_t)_ftelli64(s->fp);
#else
    return (int64_t)ftell(s->fp);
#endif
}

int64_t rt_file_stream_get_length(void* handle) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp) return 0;
#ifdef _WIN32
    int64_t cur = (int64_t)_ftelli64(s->fp);
#else
    int64_t cur = (int64_t)ftell(s->fp);
#endif
    if (fseek(s->fp, 0, SEEK_END) != 0) return 0;
#ifdef _WIN32
    int64_t end = (int64_t)_ftelli64(s->fp);
#else
    int64_t end = (int64_t)ftell(s->fp);
#endif
    fseek(s->fp, (long)cur, SEEK_SET);
    return end < 0 ? 0 : end;
}

int64_t rt_file_stream_get_position(void* handle) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp) return 0;
#ifdef _WIN32
    int64_t p = (int64_t)_ftelli64(s->fp);
#else
    int64_t p = (int64_t)ftell(s->fp);
#endif
    return p < 0 ? 0 : p;
}

void rt_file_stream_set_position(void* handle, int64_t value) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp || value < 0) return;
    fseek(s->fp, (long)value, SEEK_SET);
}

void rt_file_stream_set_length(void* handle, int64_t value) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp || value < 0) return;
    /* 最小实现：截断到 value（写模式）。读模式忽略。 */
    if (s->mode == 0) return;
#ifdef _WIN32
    _chsize_s(_fileno(s->fp), value);
#else
    ftruncate(fileno(s->fp), (off_t)value);
#endif
}

void rt_file_stream_flush(void* handle) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed || !s->fp) return;
    fflush(s->fp);
}

int32_t rt_file_stream_can_read(void* handle) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed) return 0;
    return s->mode == 0 ? 1 : 0;
}

int32_t rt_file_stream_can_write(void* handle) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed) return 0;
    return s->mode != 0 ? 1 : 0;
}

int32_t rt_file_stream_can_seek(void* handle) {
    RtFileStream* s = rt_file_stream_from(handle);
    if (!s || s->closed) return 0;
    return 1;
}
