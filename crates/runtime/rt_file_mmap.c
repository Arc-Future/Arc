//! Memory-mapped file read-only ABI (RFC 037 M-CE1 blocker).
//!
//! Zero-copy read path for large documents. Arc facade: MemoryMappedFile.as.
//! CodeEditor MUST NOT use rt_read_file / ReadAllText for large files.

#include "rt_abi.h"

#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#else
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

typedef struct RtFileMmap {
#ifdef _WIN32
    HANDLE file;
    HANDLE mapping;
#endif
#ifndef _WIN32
    int fd;
#endif
    const char* base;
    int64_t length;
} RtFileMmap;

static RtFileMmap* rt_file_mmap_from(void* handle) {
    return (RtFileMmap*)handle;
}

void* rt_file_mmap_open(const char* path) {
    if (!path || !path[0]) {
        return NULL;
    }

#ifdef _WIN32
    HANDLE file = CreateFileA(
        path,
        GENERIC_READ,
        FILE_SHARE_READ,
        NULL,
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        NULL);
    if (file == INVALID_HANDLE_VALUE) {
        return NULL;
    }

    LARGE_INTEGER size;
    if (!GetFileSizeEx(file, &size) || size.QuadPart < 0) {
        CloseHandle(file);
        return NULL;
    }

    if (size.QuadPart == 0) {
        RtFileMmap* empty = (RtFileMmap*)calloc(1, sizeof(RtFileMmap));
        if (!empty) {
            CloseHandle(file);
            return NULL;
        }
        empty->file = file;
        empty->base = "";
        empty->length = 0;
        return empty;
    }

    HANDLE mapping = CreateFileMappingA(file, NULL, PAGE_READONLY, 0, 0, NULL);
    if (!mapping) {
        CloseHandle(file);
        return NULL;
    }

    const char* view = (const char*)MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, 0);
    if (!view) {
        CloseHandle(mapping);
        CloseHandle(file);
        return NULL;
    }

    RtFileMmap* mmap = (RtFileMmap*)calloc(1, sizeof(RtFileMmap));
    if (!mmap) {
        UnmapViewOfFile(view);
        CloseHandle(mapping);
        CloseHandle(file);
        return NULL;
    }
    mmap->file = file;
    mmap->mapping = mapping;
    mmap->base = view;
    mmap->length = size.QuadPart;
    return mmap;
#else
    int fd = open(path, O_RDONLY);
    if (fd < 0) {
        return NULL;
    }

    struct stat st;
    if (fstat(fd, &st) != 0 || st.st_size < 0) {
        close(fd);
        return NULL;
    }

    int64_t length = (int64_t)st.st_size;
    if (length == 0) {
        RtFileMmap* empty = (RtFileMmap*)calloc(1, sizeof(RtFileMmap));
        if (!empty) {
            close(fd);
            return NULL;
        }
        empty->fd = fd;
        empty->base = "";
        empty->length = 0;
        return empty;
    }

    void* view = mmap(NULL, (size_t)length, PROT_READ, MAP_PRIVATE, fd, 0);
    if (view == MAP_FAILED) {
        close(fd);
        return NULL;
    }

    RtFileMmap* mmap = (RtFileMmap*)calloc(1, sizeof(RtFileMmap));
    if (!mmap) {
        munmap(view, (size_t)length);
        close(fd);
        return NULL;
    }
    mmap->fd = fd;
    mmap->base = (const char*)view;
    mmap->length = length;
    return mmap;
#endif
}

void rt_file_mmap_close(void* handle) {
    RtFileMmap* mmap = rt_file_mmap_from(handle);
    if (!mmap) {
        return;
    }

#ifdef _WIN32
    if (mmap->base && mmap->length > 0) {
        UnmapViewOfFile(mmap->base);
    }
    if (mmap->mapping) {
        CloseHandle(mmap->mapping);
    }
    if (mmap->file && mmap->file != INVALID_HANDLE_VALUE) {
        CloseHandle(mmap->file);
    }
#else
    if (mmap->base && mmap->length > 0) {
        munmap((void*)mmap->base, (size_t)mmap->length);
    }
    if (mmap->fd >= 0) {
        close(mmap->fd);
    }
#endif

    free(mmap);
}

int64_t rt_file_mmap_length(void* handle) {
    RtFileMmap* mmap = rt_file_mmap_from(handle);
    if (!mmap) {
        return 0;
    }
    return mmap->length;
}

const char* rt_file_mmap_data(void* handle) {
    RtFileMmap* mmap = rt_file_mmap_from(handle);
    if (!mmap) {
        return "";
    }
    if (!mmap->base) {
        return "";
    }
    return mmap->base;
}
