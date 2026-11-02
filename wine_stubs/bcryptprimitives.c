/* Wine-friendly stub: Rust std imports ProcessPrng from bcryptprimitives.dll. */
#include <windows.h>
#include <wincrypt.h>

BOOL WINAPI ProcessPrng(PVOID pbBuffer, SIZE_T cbBuffer)
{
    HCRYPTPROV prov = 0;
    if (CryptAcquireContextW(&prov, NULL, NULL, PROV_RSA_FULL, CRYPT_VERIFYCONTEXT)) {
        BOOL ok = CryptGenRandom(prov, (DWORD)cbBuffer, (BYTE *)pbBuffer);
        CryptReleaseContext(prov, 0);
        if (ok) return TRUE;
    }
    /* weak fallback — enough for Rust RNG seeding under Wine */
    {
        DWORD t = GetTickCount();
        SIZE_T i;
        BYTE *b = (BYTE *)pbBuffer;
        for (i = 0; i < cbBuffer; i++) {
            t = t * 1103515245u + 12345u + (DWORD)i;
            b[i] = (BYTE)(t >> 16);
        }
    }
    return TRUE;
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID r)
{
    (void)h; (void)reason; (void)r;
    return TRUE;
}
