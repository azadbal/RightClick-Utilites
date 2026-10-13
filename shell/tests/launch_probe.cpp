#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <filesystem>
#include <fstream>
#include <string>
int WINAPI wWinMain(HINSTANCE, HINSTANCE, PWSTR, int) {
    wchar_t log[32768];
    DWORD length = GetEnvironmentVariableW(L"UTILITY_SHELL_TEST_LOG", log, ARRAYSIZE(log));
    if (!length || length >= ARRAYSIZE(log)) return 1;
    int count;
    auto args = CommandLineToArgvW(GetCommandLineW(), &count);
    if (!args) return 1;
    std::ofstream output(std::filesystem::path(log), std::ios::binary);
    for (int index = 1; index < count; ++index) {
        int bytes = WideCharToMultiByte(CP_UTF8, 0, args[index], -1, nullptr, 0, nullptr, nullptr);
        std::string text(bytes, '\0');
        WideCharToMultiByte(CP_UTF8, 0, args[index], -1, text.data(), bytes, nullptr, nullptr);
        output << text.substr(0, bytes - 1) << '\n';
    }
    LocalFree(args);
    return 0;
}
