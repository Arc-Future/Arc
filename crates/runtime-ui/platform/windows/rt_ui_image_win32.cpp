/* RFC 026 M3.5 Win32 WIC decode (Draft).
 * 合 hub 时迁 native/platform/windows/（native tip 8e8fd488）。 */
#include "../common/rt_ui_image.h"
#include <stdlib.h>
#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <objbase.h>
#include <wincodec.h>
#pragma comment(lib, "windowscodecs.lib")
#pragma comment(lib, "ole32.lib")
static void rt_ui_wic_init_once(void) {
    static int ok = 0; if (!ok) { CoInitializeEx(NULL, COINIT_APARTMENTTHREADED); ok = 1; }
}
RtUiBitmap* rt_ui_image_load(const char* path) {
    if (!path || !path[0]) return rt_ui_image_make_failed(path);
    rt_ui_wic_init_once();
    int wlen = MultiByteToWideChar(CP_UTF8, 0, path, -1, NULL, 0);
    if (wlen <= 0) return rt_ui_image_make_failed(path);
    WCHAR* wpath = (WCHAR*)malloc((size_t)wlen * sizeof(WCHAR));
    if (!wpath) return rt_ui_image_make_failed(path);
    MultiByteToWideChar(CP_UTF8, 0, path, -1, wpath, wlen);
    IWICImagingFactory* factory = NULL;
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, NULL, CLSCTX_INPROC_SERVER,
                              IID_IWICImagingFactory, (void**)&factory)) || !factory) {
        free(wpath); return rt_ui_image_make_failed(path);
    }
    IWICBitmapDecoder* decoder = NULL;
    HRESULT hr = factory->CreateDecoderFromFilename(wpath, NULL, GENERIC_READ,
        WICDecodeMetadataCacheOnLoad, &decoder);
    free(wpath);
    if (FAILED(hr) || !decoder) { factory->Release(); return rt_ui_image_make_failed(path); }
    IWICBitmapFrameDecode* frame = NULL;
    hr = decoder->GetFrame(0, &frame); decoder->Release();
    if (FAILED(hr) || !frame) { factory->Release(); return rt_ui_image_make_failed(path); }
    UINT iw = 0, ih = 0; frame->GetSize(&iw, &ih);
    if (iw == 0 || ih == 0) { frame->Release(); factory->Release(); return rt_ui_image_make_failed(path); }
    IWICFormatConverter* converter = NULL;
    hr = factory->CreateFormatConverter(&converter);
    if (FAILED(hr) || !converter) { frame->Release(); factory->Release(); return rt_ui_image_make_failed(path); }
    hr = converter->Initialize(frame, GUID_WICPixelFormat32bppBGRA,
        WICBitmapDitherTypeNone, NULL, 0.0, WICBitmapPaletteTypeCustom);
    frame->Release();
    if (FAILED(hr)) { converter->Release(); factory->Release(); return rt_ui_image_make_failed(path); }
    RtUiBitmap* bmp = (RtUiBitmap*)calloc(1, sizeof(RtUiBitmap));
    if (!bmp) { converter->Release(); factory->Release(); return rt_ui_image_make_failed(path); }
    bmp->width = (int32_t)iw; bmp->height = (int32_t)ih; bmp->loaded = 1;
    size_t n = (size_t)iw * ih;
    bmp->pixels = (uint32_t*)malloc(n * 4);
    if (!bmp->pixels) { free(bmp); converter->Release(); factory->Release(); return rt_ui_image_make_failed(path); }
    hr = converter->CopyPixels(NULL, (UINT)(iw * 4), (UINT)(n * 4), (BYTE*)bmp->pixels);
    converter->Release(); factory->Release();
    if (FAILED(hr)) { rt_ui_image_release(bmp); return rt_ui_image_make_failed(path); }
    return bmp;
}
#else
RtUiBitmap* rt_ui_image_load(const char* path) { return rt_ui_image_make_failed(path); }
#endif
