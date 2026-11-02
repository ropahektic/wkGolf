/* Wine-friendly stub: Rust std imports WaitOnAddress from this API-set DLL. */
#include <windows.h>
#include <string.h>

BOOL WINAPI WaitOnAddress(volatile VOID *Address, PVOID CompareAddress, SIZE_T AddressSize, DWORD dwMilliseconds)
{
    typedef BOOL (WINAPI *fn_t)(volatile VOID *, PVOID, SIZE_T, DWORD);
    static fn_t real;
    static int resolved;
    if (!resolved) {
        HMODULE k = GetModuleHandleA("kernel32.dll");
        if (k) real = (fn_t)GetProcAddress(k, "WaitOnAddress");
        resolved = 1;
    }
    if (real)
        return real(Address, CompareAddress, AddressSize, dwMilliseconds);

    DWORD start = GetTickCount();
    for (;;) {
        BOOL same = FALSE;
        switch (AddressSize) {
        case 1: same = (*(volatile BYTE *)Address == *(BYTE *)CompareAddress); break;
        case 2: same = (*(volatile WORD *)Address == *(WORD *)CompareAddress); break;
        case 4: same = (*(volatile DWORD *)Address == *(DWORD *)CompareAddress); break;
        case 8: same = (*(volatile ULONGLONG *)Address == *(ULONGLONG *)CompareAddress); break;
        default:
            same = memcmp((const void *)Address, CompareAddress, AddressSize) == 0;
            break;
        }
        if (!same)
            return TRUE;
        if (dwMilliseconds != INFINITE) {
            DWORD elapsed = GetTickCount() - start;
            if (elapsed >= dwMilliseconds) {
                SetLastError(ERROR_TIMEOUT);
                return FALSE;
            }
        }
        Sleep(1);
    }
}

void WINAPI WakeByAddressSingle(PVOID Address)
{
    typedef void (WINAPI *fn_t)(PVOID);
    static fn_t real;
    static int resolved;
    (void)Address;
    if (!resolved) {
        HMODULE k = GetModuleHandleA("kernel32.dll");
        if (k) real = (fn_t)GetProcAddress(k, "WakeByAddressSingle");
        resolved = 1;
    }
    if (real) real(Address);
}

void WINAPI WakeByAddressAll(PVOID Address)
{
    typedef void (WINAPI *fn_t)(PVOID);
    static fn_t real;
    static int resolved;
    (void)Address;
    if (!resolved) {
        HMODULE k = GetModuleHandleA("kernel32.dll");
        if (k) real = (fn_t)GetProcAddress(k, "WakeByAddressAll");
        resolved = 1;
    }
    if (real) real(Address);
}

BOOL WINAPI DllMain(HINSTANCE h, DWORD reason, LPVOID r)
{
    (void)h; (void)reason; (void)r;
    return TRUE;
}
