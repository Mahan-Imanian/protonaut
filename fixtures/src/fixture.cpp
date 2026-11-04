// Controlled Windows test programs for SteamOS Preflight. One source, one
// behaviour per FX_MODE (see fixtures/README.md). Build with
// scripts/build-fixtures.sh; the output is byte-for-byte reproducible.
#include <windows.h>
#include <d3d11_1.h>
#include <dxgi.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>

#define FX_RENDER_OK 1
#define FX_BLACK 2
#define FX_CRASH 3
#define FX_EXIT_FAIL 4
#define FX_MISSING_ASSET 5
#define FX_LETTERBOX 6
#define FX_DIALOG 7
#define FX_MISSING_DLL 8
#define FX_SCENE 9
#define FX_TEXT_ERROR 10

#ifndef FX_MODE
#error "define FX_MODE"
#endif

#if FX_MODE == FX_MISSING_DLL
extern "C" __declspec(dllimport) int fx_absent_entry(void);
#endif

static LRESULT CALLBACK wnd_proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    if (m == WM_DESTROY) {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

#if FX_MODE == FX_TEXT_ERROR
/* Keeps running and shows a fatal message inside its own window, as a game
 * engine does when its data is missing. */
static LRESULT CALLBACK text_proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    if (m == WM_PAINT) {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(h, &ps);
        RECT r;
        GetClientRect(h, &r);
        FillRect(dc, &r, (HBRUSH)GetStockObject(BLACK_BRUSH));
        HFONT font = CreateFontW(40, 0, 0, 0, FW_NORMAL, 0, 0, 0, DEFAULT_CHARSET, 0, 0, ANTIALIASED_QUALITY, 0, L"Arial");
        HGDIOBJ old = SelectObject(dc, font);
        SetTextColor(dc, RGB(255, 255, 255));
        SetBkMode(dc, TRANSPARENT);
        DrawTextW(dc, L"Fatal error: required data files are missing", -1, &r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(dc, old);
        DeleteObject(font);
        EndPaint(h, &ps);
        return 0;
    }
    return wnd_proc(h, m, w, l);
}
#endif

static double arg_seconds(const char* cmd, double fallback) {
    const char* p = strstr(cmd, "--seconds=");
    return p ? atof(p + 10) : fallback;
}

int WINAPI WinMain(HINSTANCE inst, HINSTANCE, LPSTR cmd, int) {
#if FX_MODE == FX_MISSING_DLL
    return fx_absent_entry();
#endif
#if FX_MODE == FX_EXIT_FAIL
    fprintf(stderr, "fixture: refusing to start (exit 3)\n");
    return 3;
#endif
#if FX_MODE == FX_MISSING_ASSET
    {
        char path[MAX_PATH];
        DWORD n = GetModuleFileNameA(NULL, path, MAX_PATH);
        while (n > 0 && path[n - 1] != '\\') n--;
        path[n] = 0;
        strcat(path, "assets\\level0.dat");
        if (GetFileAttributesA(path) == INVALID_FILE_ATTRIBUTES) {
            fprintf(stderr, "fixture: missing assets\\level0.dat\n");
            return 4;
        }
    }
#endif
    const double seconds = arg_seconds(cmd, 3600.0);
    const int width = GetSystemMetrics(SM_CXSCREEN);
    const int height = GetSystemMetrics(SM_CYSCREEN);
#if FX_MODE == FX_TEXT_ERROR
    {
        WNDCLASSW tc = {};
        tc.lpfnWndProc = text_proc;
        tc.hInstance = inst;
        tc.lpszClassName = L"preflight_text";
        RegisterClassW(&tc);
        CreateWindowW(L"preflight_text", L"Preflight Fixture", WS_POPUP | WS_VISIBLE, 0, 0, width, height, 0, 0, inst, 0);
        const DWORD t0 = GetTickCount();
        MSG msg;
        while ((GetTickCount() - t0) / 1000.0 < seconds) {
            while (PeekMessageW(&msg, 0, 0, 0, PM_REMOVE)) {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            Sleep(16);
        }
        return 0;
    }
#endif

    WNDCLASSW wc = {};
    wc.lpfnWndProc = wnd_proc;
    wc.hInstance = inst;
    wc.lpszClassName = L"preflight_fixture";
    RegisterClassW(&wc);
    HWND hwnd = CreateWindowW(L"preflight_fixture", L"Preflight Fixture", WS_POPUP | WS_VISIBLE, 0, 0, width, height, 0, 0, inst, 0);
#if FX_MODE == FX_DIALOG
    CreateWindowW(L"preflight_fixture", L"Unsupported graphics card", WS_POPUP | WS_VISIBLE | WS_BORDER, 400, 300, 420, 160, 0, 0, inst, 0);
#endif

    DXGI_SWAP_CHAIN_DESC sd = {};
    sd.BufferCount = 2;
    sd.BufferDesc.Width = width;
    sd.BufferDesc.Height = height;
    sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.OutputWindow = hwnd;
    sd.SampleDesc.Count = 1;
    sd.Windowed = TRUE;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    IDXGISwapChain* swap = NULL;
    ID3D11Device* dev = NULL;
    ID3D11DeviceContext* ctx = NULL;
    D3D_FEATURE_LEVEL level;
    HRESULT hr = D3D11CreateDeviceAndSwapChain(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0, D3D11_SDK_VERSION, &sd, &swap, &dev, &level, &ctx);
    if (FAILED(hr)) {
        fprintf(stderr, "fixture: D3D11CreateDeviceAndSwapChain failed 0x%08lx\n", (unsigned long)hr);
        return 2;
    }
    if (strstr(cmd, "--report")) {
        IDXGIDevice* xdev = NULL;
        IDXGIAdapter* adapter = NULL;
        DXGI_ADAPTER_DESC desc;
        FILE* f = fopen("result.txt", "w");
        if (f && SUCCEEDED(dev->QueryInterface(__uuidof(IDXGIDevice), (void**)&xdev)) && SUCCEEDED(xdev->GetAdapter(&adapter)) && SUCCEEDED(adapter->GetDesc(&desc))) {
            fprintf(f, "feature_level=0x%x\nadapter=%ls\n", (unsigned)level, desc.Description);
        }
        if (f) fclose(f);
    }
    ID3D11Texture2D* back = NULL;
    swap->GetBuffer(0, __uuidof(ID3D11Texture2D), (void**)&back);
    ID3D11RenderTargetView* rtv = NULL;
    dev->CreateRenderTargetView(back, NULL, &rtv);
    ID3D11DeviceContext1* ctx1 = NULL;
    ctx->QueryInterface(__uuidof(ID3D11DeviceContext1), (void**)&ctx1);

#if FX_MODE == FX_BLACK
    const float color[4] = {0.0f, 0.0f, 0.0f, 1.0f};
#else
    const float color[4] = {0.1f, 0.6f, 0.2f, 1.0f};
#endif
    const float black[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    const DWORD start = GetTickCount();
    int frames = 0;
    for (;;) {
        MSG msg;
        while (PeekMessageW(&msg, 0, 0, 0, PM_REMOVE)) {
            if (msg.message == WM_QUIT) return 0;
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        ctx->OMSetRenderTargets(1, &rtv, NULL);
#if FX_MODE == FX_LETTERBOX
        ctx->ClearRenderTargetView(rtv, black);
        if (ctx1) {
            // A 16:9 picture inside a 16:10 screen: bars at top and bottom.
            const int content = width * 9 / 16;
            D3D11_RECT r = {0, (height - content) / 2, width, (height - content) / 2 + content};
            ctx1->ClearView(rtv, color, &r, 1);
        }
#elif FX_MODE == FX_SCENE
        // A stand-in for a real game frame: a background with a grid of
        // differently coloured tiles, so the frame is neither black nor flat.
        ctx->ClearRenderTargetView(rtv, color);
        if (ctx1) {
            for (int ty = 0; ty < 4; ty++) {
                for (int tx = 0; tx < 6; tx++) {
                    const float tile[4] = {tx / 6.0f, ty / 4.0f, 0.8f, 1.0f};
                    D3D11_RECT r = {tx * width / 6 + 8, ty * height / 4 + 8, (tx + 1) * width / 6 - 8, (ty + 1) * height / 4 - 8};
                    ctx1->ClearView(rtv, tile, &r, 1);
                }
            }
        }
        (void)black;
#else
        (void)black;
        ctx->ClearRenderTargetView(rtv, color);
#endif
        swap->Present(1, 0);
        frames++;
#if FX_MODE == FX_CRASH
        if (frames == 180) {
            volatile int zero = 0;
            volatile int boom = frames / zero;
            (void)boom;
        }
#endif
        if ((GetTickCount() - start) / 1000.0 >= seconds) break;
    }
    return 0;
}
