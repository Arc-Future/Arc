//! Piece-table text buffer for CodeEditor (RFC 037 M-CE1).
//!
//! - Original buffer: mmap read-only (zero-copy)
//! - Add buffer: append-only edits
//! - Line index: eager <= 64 MB; chunked lazy 256 KB blocks above

#include "../runtime/rt_abi.h"

#include <stdlib.h>
#include <string.h>

#define RT_EDITOR_EAGER_LIMIT ((int64_t)(64 * 1024 * 1024))
#define RT_EDITOR_CHUNK_SIZE (256 * 1024)
#define RT_EDITOR_PIECE_ORIGINAL 0
#define RT_EDITOR_PIECE_ADD 1

typedef struct {
    int32_t source;
    int64_t start;
    int64_t length;
} RtEditorPiece;

typedef struct {
    int64_t byte_offset;
    int64_t byte_length;
    int32_t first_line;
    int32_t line_count;
    int64_t* line_starts;
    int32_t built;
} RtEditorChunk;

typedef struct {
    void* mmap_handle;
    const char* original;
    int64_t original_len;

    char* add_data;
    int64_t add_len;
    int64_t add_cap;

    RtEditorPiece* pieces;
    int32_t piece_count;
    int32_t piece_cap;
    int64_t total_len;

    int32_t line_count;
    int32_t index_mode;
    int64_t* eager_line_starts;
    RtEditorChunk* chunks;
    int32_t chunk_count;
} RtEditorDoc;

static RtEditorDoc* rt_editor_from(void* handle) {
    return (RtEditorDoc*)handle;
}

static int64_t editor_piece_end(RtEditorPiece* p) {
    return p->start + p->length;
}

static int32_t editor_grow_pieces(RtEditorDoc* doc) {
    if (doc->piece_count < doc->piece_cap) {
        return 1;
    }
    int32_t new_cap = doc->piece_cap == 0 ? 8 : doc->piece_cap * 2;
    RtEditorPiece* np = (RtEditorPiece*)realloc(doc->pieces, (size_t)new_cap * sizeof(RtEditorPiece));
    if (!np) {
        return 0;
    }
    doc->pieces = np;
    doc->piece_cap = new_cap;
    return 1;
}

static int32_t editor_grow_add(RtEditorDoc* doc, int64_t need) {
    if (need <= doc->add_cap) {
        return 1;
    }
    int64_t new_cap = doc->add_cap == 0 ? 4096 : doc->add_cap;
    while (new_cap < need) {
        new_cap *= 2;
    }
    char* nd = (char*)realloc(doc->add_data, (size_t)new_cap);
    if (!nd) {
        return 0;
    }
    doc->add_data = nd;
    doc->add_cap = new_cap;
    return 1;
}

static const char* editor_source_ptr(RtEditorDoc* doc, RtEditorPiece* p) {
    if (p->source == RT_EDITOR_PIECE_ORIGINAL) {
        return doc->original + p->start;
    }
    return doc->add_data + p->start;
}

static void editor_recompute_total(RtEditorDoc* doc) {
    int64_t total = 0;
    for (int32_t i = 0; i < doc->piece_count; i++) {
        total += doc->pieces[i].length;
    }
    doc->total_len = total;
}

static int32_t editor_count_newlines(const char* data, int64_t len) {
    int32_t count = 0;
    for (int64_t i = 0; i < len; i++) {
        if (data[i] == '\n') {
            count++;
        }
    }
    if (len > 0) {
        count++;
    }
    return count;
}

static void editor_scan_lines(const char* data, int64_t len, int64_t base_offset,
                              int64_t** out_starts, int32_t* out_count) {
    int32_t cap = 64;
    int32_t count = 0;
    int64_t* starts = (int64_t*)malloc((size_t)cap * sizeof(int64_t));
    if (!starts) {
        *out_starts = NULL;
        *out_count = 0;
        return;
    }

    if (len > 0) {
        starts[count++] = base_offset;
        if (cap <= count) {
            cap *= 2;
            int64_t* ns = (int64_t*)realloc(starts, (size_t)cap * sizeof(int64_t));
            if (!ns) {
                free(starts);
                *out_starts = NULL;
                *out_count = 0;
                return;
            }
            starts = ns;
        }
    }

    for (int64_t i = 0; i < len; i++) {
        if (data[i] == '\n' && i + 1 < len) {
            if (count >= cap) {
                cap *= 2;
                int64_t* ns = (int64_t*)realloc(starts, (size_t)cap * sizeof(int64_t));
                if (!ns) {
                    free(starts);
                    *out_starts = NULL;
                    *out_count = 0;
                    return;
                }
                starts = ns;
            }
            starts[count++] = base_offset + i + 1;
        }
    }

    *out_starts = starts;
    *out_count = count;
}

static void editor_build_eager_index(RtEditorDoc* doc) {
    doc->index_mode = 0;
    if (doc->total_len == 0) {
        doc->line_count = 0;
        doc->eager_line_starts = NULL;
        return;
    }

    int64_t* starts = NULL;
    int32_t count = 0;

    int64_t pos = 0;
    for (int32_t i = 0; i < doc->piece_count; i++) {
        RtEditorPiece* p = &doc->pieces[i];
        const char* data = editor_source_ptr(doc, p);
        int64_t* piece_starts = NULL;
        int32_t piece_count = 0;
        editor_scan_lines(data, p->length, pos, &piece_starts, &piece_count);
        if (piece_count == 0 && p->length > 0) {
            piece_starts = (int64_t*)malloc(sizeof(int64_t));
            if (piece_starts) {
                piece_starts[0] = pos;
                piece_count = 1;
            }
        }
        if (piece_count > 0) {
            int64_t* merged = (int64_t*)realloc(starts, (size_t)(count + piece_count) * sizeof(int64_t));
            if (!merged) {
                free(piece_starts);
                break;
            }
            starts = merged;
            for (int32_t j = 0; j < piece_count; j++) {
                if (count == 0 || starts[count - 1] != piece_starts[j]) {
                    starts[count++] = piece_starts[j];
                }
            }
        }
        free(piece_starts);
        pos += p->length;
    }

    doc->eager_line_starts = starts;
    doc->line_count = count;
}

static void editor_init_chunks(RtEditorDoc* doc) {
    doc->index_mode = 1;
    if (doc->total_len == 0) {
        doc->chunk_count = 0;
        doc->chunks = NULL;
        doc->line_count = 0;
        return;
    }

    int32_t n = (int32_t)((doc->total_len + RT_EDITOR_CHUNK_SIZE - 1) / RT_EDITOR_CHUNK_SIZE);
    doc->chunks = (RtEditorChunk*)calloc((size_t)n, sizeof(RtEditorChunk));
    if (!doc->chunks) {
        doc->chunk_count = 0;
        doc->line_count = 0;
        return;
    }
    doc->chunk_count = n;

    for (int32_t i = 0; i < n; i++) {
        doc->chunks[i].byte_offset = (int64_t)i * RT_EDITOR_CHUNK_SIZE;
        doc->chunks[i].byte_length = RT_EDITOR_CHUNK_SIZE;
        if (doc->chunks[i].byte_offset + doc->chunks[i].byte_length > doc->total_len) {
            doc->chunks[i].byte_length = doc->total_len - doc->chunks[i].byte_offset;
        }
        doc->chunks[i].first_line = -1;
        doc->chunks[i].line_count = 0;
        doc->chunks[i].line_starts = NULL;
        doc->chunks[i].built = 0;
    }

    doc->line_count = editor_count_newlines(doc->original, doc->original_len);
}

static void editor_read_bytes(RtEditorDoc* doc, int64_t offset, int64_t len, char* out) {
    int64_t written = 0;
    int64_t pos = 0;
    for (int32_t i = 0; i < doc->piece_count && written < len; i++) {
        RtEditorPiece* p = &doc->pieces[i];
        int64_t piece_end = pos + p->length;
        if (offset >= piece_end) {
            pos = piece_end;
            continue;
        }
        int64_t skip = offset - pos;
        if (skip < 0) {
            skip = 0;
        }
        int64_t avail = p->length - skip;
        int64_t take = len - written;
        if (take > avail) {
            take = avail;
        }
        const char* src = editor_source_ptr(doc, p) + skip;
        memcpy(out + written, src, (size_t)take);
        written += take;
        offset += take;
        pos = piece_end;
    }
    if (written < len) {
        memset(out + written, 0, (size_t)(len - written));
    }
}

static int64_t editor_line_start(RtEditorDoc* doc, int32_t line_no) {
    if (line_no < 0) {
        return 0;
    }
    if (doc->index_mode == 0) {
        if (!doc->eager_line_starts || line_no >= doc->line_count) {
            return doc->total_len;
        }
        return doc->eager_line_starts[line_no];
    }

    for (int32_t c = 0; c < doc->chunk_count; c++) {
        RtEditorChunk* ch = &doc->chunks[c];
        if (!ch->built) {
            continue;
        }
        if (line_no >= ch->first_line && line_no < ch->first_line + ch->line_count) {
            int32_t local = line_no - ch->first_line;
            return ch->line_starts[local];
        }
    }
    return 0;
}

static void editor_build_chunk(RtEditorDoc* doc, int32_t chunk_idx) {
    if (chunk_idx < 0 || chunk_idx >= doc->chunk_count) {
        return;
    }
    RtEditorChunk* ch = &doc->chunks[chunk_idx];
    if (ch->built) {
        return;
    }

    char* buf = (char*)malloc((size_t)ch->byte_length + 1);
    if (!buf) {
        return;
    }
    editor_read_bytes(doc, ch->byte_offset, ch->byte_length, buf);
    buf[ch->byte_length] = '\0';

    int64_t* rel = NULL;
    int32_t rel_count = 0;
    editor_scan_lines(buf, ch->byte_length, ch->byte_offset, &rel, &rel_count);
    free(buf);

    if (chunk_idx == 0) {
        ch->first_line = 0;
    } else {
        int32_t prev = chunk_idx - 1;
        while (prev >= 0 && !doc->chunks[prev].built) {
            editor_build_chunk(doc, prev);
        }
        if (prev >= 0 && doc->chunks[prev].built) {
            ch->first_line = doc->chunks[prev].first_line + doc->chunks[prev].line_count;
            if (doc->chunks[prev].line_count > 0) {
                int64_t prev_end = doc->chunks[prev].byte_offset + doc->chunks[prev].byte_length;
                if (prev_end > 0 && rel_count > 0 && rel[0] == prev_end) {
                    ch->first_line = doc->chunks[prev].first_line + doc->chunks[prev].line_count - 1;
                    memmove(rel, rel + 1, (size_t)(rel_count - 1) * sizeof(int64_t));
                    rel_count--;
                }
            }
        } else {
            ch->first_line = 0;
        }
    }

    ch->line_starts = rel;
    ch->line_count = rel_count;
    ch->built = 1;
}

static void editor_destroy_index(RtEditorDoc* doc) {
    free(doc->eager_line_starts);
    doc->eager_line_starts = NULL;
    if (doc->chunks) {
        for (int32_t i = 0; i < doc->chunk_count; i++) {
            free(doc->chunks[i].line_starts);
        }
        free(doc->chunks);
    }
    doc->chunks = NULL;
    doc->chunk_count = 0;
}

static void editor_destroy_doc(RtEditorDoc* doc) {
    if (!doc) {
        return;
    }
    if (doc->mmap_handle) {
        rt_file_mmap_close(doc->mmap_handle);
    }
    free(doc->add_data);
    free(doc->pieces);
    editor_destroy_index(doc);
    free(doc);
}

static RtEditorDoc* editor_create_base(void) {
    RtEditorDoc* doc = (RtEditorDoc*)calloc(1, sizeof(RtEditorDoc));
    return doc;
}

static int32_t editor_set_single_add_piece(RtEditorDoc* doc, const char* text) {
    doc->mmap_handle = NULL;
    doc->original = "";
    doc->original_len = 0;
    doc->add_len = 0;
    doc->piece_count = 0;

    if (!text) {
        text = "";
    }
    int64_t len = (int64_t)strlen(text);
    if (!editor_grow_add(doc, len + 1)) {
        return 0;
    }
    if (len > 0) {
        memcpy(doc->add_data, text, (size_t)len);
    }
    doc->add_len = len;

    if (!editor_grow_pieces(doc)) {
        return 0;
    }
    doc->pieces[0].source = RT_EDITOR_PIECE_ADD;
    doc->pieces[0].start = 0;
    doc->pieces[0].length = len;
    doc->piece_count = 1;
    editor_recompute_total(doc);

    if (doc->total_len <= RT_EDITOR_EAGER_LIMIT) {
        editor_build_eager_index(doc);
    } else {
        doc->index_mode = 1;
        editor_init_chunks(doc);
    }
    return 1;
}

void* rt_editor_create_empty(void) {
    RtEditorDoc* doc = editor_create_base();
    if (!doc) {
        return NULL;
    }
    if (!editor_set_single_add_piece(doc, "")) {
        editor_destroy_doc(doc);
        return NULL;
    }
    return doc;
}

void* rt_editor_open_path(const char* path) {
    if (!path || !path[0]) {
        return NULL;
    }

    void* mmap = rt_file_mmap_open(path);
    if (!mmap) {
        return NULL;
    }

    int64_t len = rt_file_mmap_length(mmap);
    const char* data = rt_file_mmap_data(mmap);

    RtEditorDoc* doc = editor_create_base();
    if (!doc) {
        rt_file_mmap_close(mmap);
        return NULL;
    }

    doc->mmap_handle = mmap;
    doc->original = data ? data : "";
    doc->original_len = len;

    if (!editor_grow_pieces(doc)) {
        editor_destroy_doc(doc);
        return NULL;
    }
    doc->pieces[0].source = RT_EDITOR_PIECE_ORIGINAL;
    doc->pieces[0].start = 0;
    doc->pieces[0].length = len;
    doc->piece_count = 1;
    editor_recompute_total(doc);

    if (doc->total_len <= RT_EDITOR_EAGER_LIMIT) {
        editor_build_eager_index(doc);
    } else {
        editor_init_chunks(doc);
    }

    return doc;
}

void rt_editor_destroy(void* handle) {
    editor_destroy_doc(rt_editor_from(handle));
}

int64_t rt_editor_length(void* handle) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc) {
        return 0;
    }
    return doc->total_len;
}

int32_t rt_editor_line_count(void* handle) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc) {
        return 0;
    }
    return doc->line_count;
}

int32_t rt_editor_ensure_lines(void* handle, int32_t first_line, int32_t last_line) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc) {
        return 0;
    }
    if (doc->index_mode == 0) {
        return 1;
    }
    if (first_line < 0) {
        first_line = 0;
    }
    if (last_line < first_line) {
        last_line = first_line;
    }
    if (last_line >= doc->line_count) {
        last_line = doc->line_count - 1;
    }

    for (int32_t c = 0; c < doc->chunk_count; c++) {
        if (!doc->chunks[c].built) {
            editor_build_chunk(doc, c);
        }
        if (doc->chunks[c].built && doc->chunks[c].first_line + doc->chunks[c].line_count > last_line) {
            break;
        }
    }
    return 1;
}

char* rt_editor_line_text(void* handle, int32_t line_no) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc || line_no < 0 || line_no >= doc->line_count) {
        char* empty = (char*)malloc(1);
        if (empty) {
            empty[0] = '\0';
        }
        return empty;
    }

    rt_editor_ensure_lines(handle, line_no, line_no);

    int64_t start = editor_line_start(doc, line_no);
    int64_t end = doc->total_len;
    if (line_no + 1 < doc->line_count) {
        end = editor_line_start(doc, line_no + 1);
        if (end > start && end <= doc->total_len) {
            char last;
            editor_read_bytes(doc, end - 1, 1, &last);
            if (last == '\n') {
                end--;
            }
        }
    }

    int64_t len = end - start;
    if (len < 0) {
        len = 0;
    }
    char* out = (char*)malloc((size_t)len + 1);
    if (!out) {
        return NULL;
    }
    if (len > 0) {
        editor_read_bytes(doc, start, len, out);
    }
    out[len] = '\0';
    return out;
}

int32_t rt_editor_set_text(void* handle, const char* text) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc) {
        return 0;
    }
    if (doc->mmap_handle) {
        rt_file_mmap_close(doc->mmap_handle);
        doc->mmap_handle = NULL;
    }
    free(doc->add_data);
    doc->add_data = NULL;
    doc->add_len = 0;
    doc->add_cap = 0;
    free(doc->pieces);
    doc->pieces = NULL;
    doc->piece_count = 0;
    doc->piece_cap = 0;
    editor_destroy_index(doc);
    doc->line_count = 0;

    return editor_set_single_add_piece(doc, text);
}

int32_t rt_editor_insert(void* handle, int64_t offset, const char* text) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc || !text) {
        return 0;
    }
    int64_t tlen = (int64_t)strlen(text);
    if (tlen == 0) {
        return 1;
    }
    if (offset < 0) {
        offset = 0;
    }
    if (offset > doc->total_len) {
        offset = doc->total_len;
    }

    int64_t add_start = doc->add_len;
    if (!editor_grow_add(doc, doc->add_len + tlen + 1)) {
        return 0;
    }
    memcpy(doc->add_data + add_start, text, (size_t)tlen);
    doc->add_len += tlen;

    RtEditorPiece new_piece;
    new_piece.source = RT_EDITOR_PIECE_ADD;
    new_piece.start = add_start;
    new_piece.length = tlen;

    int64_t pos = 0;
    int32_t insert_at = doc->piece_count;
    for (int32_t i = 0; i < doc->piece_count; i++) {
        int64_t end = pos + doc->pieces[i].length;
        if (offset <= pos) {
            insert_at = i;
            break;
        }
        if (offset < end) {
            int64_t split = offset - pos;
            RtEditorPiece* p = &doc->pieces[i];
            RtEditorPiece tail = *p;
            tail.start += split;
            tail.length -= split;
            p->length = split;

            if (!editor_grow_pieces(doc)) {
                return 0;
            }
            memmove(&doc->pieces[i + 2], &doc->pieces[i + 1],
                    (size_t)(doc->piece_count - i - 1) * sizeof(RtEditorPiece));
            doc->pieces[i + 1] = new_piece;
            doc->pieces[i + 2] = tail;
            doc->piece_count += 2;
            editor_recompute_total(doc);
            editor_destroy_index(doc);
            if (doc->total_len <= RT_EDITOR_EAGER_LIMIT) {
                editor_build_eager_index(doc);
            } else {
                editor_init_chunks(doc);
            }
            return 1;
        }
        pos = end;
        insert_at = i + 1;
    }

    if (!editor_grow_pieces(doc)) {
        return 0;
    }
    if (insert_at < doc->piece_count) {
        memmove(&doc->pieces[insert_at + 1], &doc->pieces[insert_at],
                (size_t)(doc->piece_count - insert_at) * sizeof(RtEditorPiece));
    }
    doc->pieces[insert_at] = new_piece;
    doc->piece_count++;
    editor_recompute_total(doc);
    editor_destroy_index(doc);
    if (doc->total_len <= RT_EDITOR_EAGER_LIMIT) {
        editor_build_eager_index(doc);
    } else {
        editor_init_chunks(doc);
    }
    return 1;
}

int32_t rt_editor_delete(void* handle, int64_t offset, int64_t length) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc || length <= 0) {
        return 1;
    }
    if (offset < 0) {
        offset = 0;
    }
    if (offset >= doc->total_len) {
        return 1;
    }
    if (offset + length > doc->total_len) {
        length = doc->total_len - offset;
    }

    int64_t del_end = offset + length;
    int64_t pos = 0;
    int32_t write = 0;
    for (int32_t read = 0; read < doc->piece_count; read++) {
        RtEditorPiece p = doc->pieces[read];
        int64_t pstart = pos;
        int64_t pend = pos + p.length;

        if (del_end <= pstart || offset >= pend) {
            doc->pieces[write++] = p;
        } else {
            if (offset > pstart) {
                RtEditorPiece head = p;
                head.length = offset - pstart;
                doc->pieces[write++] = head;
            }
            if (del_end < pend) {
                RtEditorPiece tail = p;
                tail.start += del_end - pstart;
                tail.length = pend - del_end;
                doc->pieces[write++] = tail;
            }
        }
        pos = pend;
    }
    doc->piece_count = write;
    editor_recompute_total(doc);
    editor_destroy_index(doc);
    if (doc->total_len <= RT_EDITOR_EAGER_LIMIT) {
        editor_build_eager_index(doc);
    } else if (doc->total_len > 0) {
        editor_init_chunks(doc);
    } else {
        doc->line_count = 0;
    }
    return 1;
}

int32_t rt_editor_is_mmap_backed(void* handle) {
    RtEditorDoc* doc = rt_editor_from(handle);
    if (!doc) {
        return 0;
    }
    return doc->mmap_handle != NULL ? 1 : 0;
}
