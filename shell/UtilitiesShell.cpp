#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shobjidl.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <wrl.h>
#include <winrt/Windows.Data.Json.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <atomic>
#include <algorithm>
#include <fstream>
#include <filesystem>
#include <vector>

using Microsoft::WRL::ComPtr;
using namespace winrt::Windows::Data::Json;
// Must match package/AppxManifest.xml.
const CLSID CLSID_Utilities = {0xa087536d,0x4ef7,0x4d4d,{0xb2,0x4e,0x4b,0xc2,0xf2,0x82,0x66,0x89}};
static HMODULE module;
static std::atomic<long> objects{0}, locks{0};

struct Function {
    std::wstring id, label, appliesTo;
    bool enabled = true;
    std::vector<std::wstring> extensions;
};
static std::wstring Lower(std::wstring text) {
    std::transform(text.begin(), text.end(), text.begin(), towlower);
    return text;
}
static std::vector<Function> Defaults() {
    std::vector<Function> result;
    for (auto pair : {std::pair{L"video-15mb", L"Compress video below 15 MB"},
                      std::pair{L"video-20mb", L"Compress video below 20 MB"},
                      std::pair{L"strip-audio", L"Remove audio from video"}}) {
        result.push_back({pair.first, pair.second, L"file", true,
            {L".mp4", L".mov", L".mkv", L".avi", L".webm", L".m4v", L".wmv", L".mpeg", L".mpg",
             L".mts", L".m2ts", L".flv", L".ts", L".3gp", L".ogv"}});
    }
    return result;
}
static std::vector<Function> ReadFunctionsAt(const std::filesystem::path& path) {
    // Explorer never writes configuration and bounds its menu-construction IO.
    std::error_code error;
    if (!std::filesystem::exists(path, error)) return error ? std::vector<Function>{} : Defaults();
    auto size = std::filesystem::file_size(path, error);
    if (error || size > 1024 * 1024) return {};
    std::ifstream input(path, std::ios::binary);
    if (!input) return {};
    std::string bytes{std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
    if (bytes.size() >= 3 && bytes.compare(0, 3, "\xef\xbb\xbf") == 0) bytes.erase(0, 3);
    try {
        auto json = JsonObject::Parse(winrt::to_hstring(bytes));
        if (json.GetNamedNumber(L"version", 0) != 1) return {};
        std::vector<Function> result;
        for (auto value : json.GetNamedArray(L"functions")) {
            auto item = value.GetObject();
            Function function;
            function.id = item.GetNamedString(L"id", L"");
            function.label = item.GetNamedString(L"label", L"");
            function.appliesTo = item.GetNamedString(L"appliesTo", L"any");
            function.enabled = item.GetNamedBoolean(L"enabled", true);
            if (item.HasKey(L"extensions")) {
                for (auto extension : item.GetNamedArray(L"extensions"))
                    function.extensions.push_back(Lower(std::wstring(extension.GetString())));
            }
            if (function.id.empty() || function.label.empty() || !function.enabled) continue;
            if (function.appliesTo != L"any" && function.appliesTo != L"file" && function.appliesTo != L"folder") continue;
            result.push_back(std::move(function));
        }
        return result;
    } catch (...) { return {}; }
}
static std::vector<Function> ReadFunctions() {
    PWSTR profile = nullptr;
    if (FAILED(SHGetKnownFolderPath(FOLDERID_Profile, 0, nullptr, &profile))) return {};
    std::filesystem::path path = std::filesystem::path(profile) / L".rightclick-utilities" / L"functions.json";
    CoTaskMemFree(profile);
    return ReadFunctionsAt(path);
}
struct Selection { std::wstring path; bool folder = false; DWORD count = 0; };
static HRESULT GetSelection(IShellItemArray* items, Selection& selection) {
    if (!items) return S_FALSE;
    HRESULT hr = items->GetCount(&selection.count);
    if (FAILED(hr)) return hr;
    if (selection.count != 1) return S_FALSE;
    ComPtr<IShellItem> item;
    if (FAILED(hr = items->GetItemAt(0, &item))) return hr;
    PWSTR path = nullptr;
    if (FAILED(hr = item->GetDisplayName(SIGDN_FILESYSPATH, &path))) return hr;
    selection.path = path;
    CoTaskMemFree(path);
    DWORD attributes = GetFileAttributesW(selection.path.c_str());
    if (attributes == INVALID_FILE_ATTRIBUTES) return HRESULT_FROM_WIN32(GetLastError());
    selection.folder = (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
    return S_OK;
}
static bool Applies(const Function& function, const Selection& selection) {
    if (function.appliesTo == L"file" && selection.folder) return false;
    if (function.appliesTo == L"folder" && !selection.folder) return false;
    if (selection.folder || function.extensions.empty()) return true;
    auto extension = Lower(std::filesystem::path(selection.path).extension().wstring());
    return std::find(function.extensions.begin(), function.extensions.end(), extension) != function.extensions.end();
}
static std::wstring ModuleDirectory() {
    wchar_t path[32768];
    DWORD size = GetModuleFileNameW(module, path, ARRAYSIZE(path));
    if (!size || size == ARRAYSIZE(path)) return {};
    return std::filesystem::path(path).parent_path().wstring();
}
// Windows argv quoting, including quotes and trailing backslashes. Never use a shell.
static std::wstring Quote(const std::wstring& value) {
    std::wstring result = L"\"";
    unsigned slashes = 0;
    for (auto character : value) {
        if (character == L'\\') { ++slashes; continue; }
        if (character == L'\"') result.append(slashes * 2 + 1, L'\\');
        else result.append(slashes, L'\\');
        result += character;
        slashes = 0;
    }
    result.append(slashes * 2, L'\\');
    return result + L"\"";
}
static HRESULT Launch(const std::wstring& id, const Selection& selection) {
    auto directory = ModuleDirectory();
    if (directory.empty()) return E_FAIL;
    auto executable = directory + L"\\Utilities.exe";
    std::wstring command = Quote(executable);
    if (!id.empty()) command += L" --run-function " + Quote(id);
    if (!selection.path.empty()) command += L" --input " + Quote(selection.path);
    STARTUPINFOW startup{sizeof(startup)};
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(executable.c_str(), command.data(), nullptr, nullptr, FALSE,
                        0, nullptr, directory.c_str(), &startup, &process))
        return HRESULT_FROM_WIN32(GetLastError());
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    return S_OK;
}

class Command;
class CommandEnum final : public Microsoft::WRL::RuntimeClass<Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>, IEnumExplorerCommand> {
    std::vector<ComPtr<IExplorerCommand>> commands;
    size_t index = 0;
public:
    CommandEnum(std::vector<ComPtr<IExplorerCommand>> values, size_t position = 0) : commands(std::move(values)), index(position) { ++objects; }
    ~CommandEnum() { --objects; }
    IFACEMETHODIMP Next(ULONG requested, IExplorerCommand** output, ULONG* fetched) override {
        if (!output || (!fetched && requested != 1)) return E_POINTER;
        ULONG count = 0;
        while (count < requested && index < commands.size()) {
            output[count] = commands[index++].Get();
            output[count++]->AddRef();
        }
        if (fetched) *fetched = count;
        return count == requested ? S_OK : S_FALSE;
    }
    IFACEMETHODIMP Skip(ULONG count) override {
        size_t remaining = commands.size() - index;
        index += (std::min)(static_cast<size_t>(count), remaining);
        return count <= remaining ? S_OK : S_FALSE;
    }
    IFACEMETHODIMP Reset() override { index = 0; return S_OK; }
    IFACEMETHODIMP Clone(IEnumExplorerCommand** output) override {
        if (!output) return E_POINTER;
        return Microsoft::WRL::Make<CommandEnum>(commands, index).CopyTo(output);
    }
};
class Command final : public Microsoft::WRL::RuntimeClass<Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>, IExplorerCommand> {
    bool root;
    Function function;
public:
    Command() : root(true) { ++objects; }
    explicit Command(Function entry) : root(false), function(std::move(entry)) { ++objects; }
    ~Command() { --objects; }
    IFACEMETHODIMP GetTitle(IShellItemArray*, PWSTR* title) override {
        return SHStrDupW(root ? L"RightClick Utilities" : function.label.c_str(), title);
    }
    IFACEMETHODIMP GetIcon(IShellItemArray*, PWSTR* icon) override {
        return SHStrDupW((ModuleDirectory() + L"\\Assets\\Utilities.ico").c_str(), icon);
    }
    IFACEMETHODIMP GetToolTip(IShellItemArray*, PWSTR* tooltip) override {
        if (!tooltip) return E_POINTER;
        *tooltip = nullptr; return E_NOTIMPL;
    }
    IFACEMETHODIMP GetCanonicalName(GUID* name) override {
        if (!name) return E_POINTER;
        *name = root ? CLSID_Utilities : GUID_NULL; return S_OK;
    }
    IFACEMETHODIMP GetState(IShellItemArray* items, BOOL, EXPCMDSTATE* state) override {
        if (!state) return E_POINTER;
        Selection selection;
        HRESULT hr = GetSelection(items, selection);
        *state = ECS_ENABLED;
        if (!items) return S_OK; // EnumSubCommands does not receive a selection.
        if (FAILED(hr)) *state = ECS_HIDDEN;
        else if (selection.count != 1) *state = ECS_DISABLED;
        else if (!root && !function.id.empty() && !Applies(function, selection)) *state = ECS_HIDDEN;
        return S_OK;
    }
    IFACEMETHODIMP Invoke(IShellItemArray* items, IBindCtx*) override {
        if (root) return E_NOTIMPL;
        Selection selection;
        HRESULT hr = GetSelection(items, selection);
        if (FAILED(hr)) return hr;
        if (items && selection.count != 1) return E_INVALIDARG;
        if (!function.id.empty()) {
            if (hr != S_OK) return E_INVALIDARG;
            auto current = ReadFunctions();
            auto found = std::find_if(current.begin(), current.end(), [&](auto& entry) { return entry.id == function.id; });
            if (found == current.end() || !Applies(*found, selection)) return E_ACCESSDENIED;
        }
        return Launch(function.id, selection);
    }
    IFACEMETHODIMP GetFlags(EXPCMDFLAGS* flags) override {
        if (!flags) return E_POINTER;
        *flags = root ? ECF_HASSUBCOMMANDS : ECF_DEFAULT; return S_OK;
    }
    IFACEMETHODIMP EnumSubCommands(IEnumExplorerCommand** output) override {
        if (!output) return E_POINTER;
        *output = nullptr;
        if (!root) return E_NOTIMPL;
        try {
            std::vector<ComPtr<IExplorerCommand>> children;
            for (auto& entry : ReadFunctions()) children.push_back(Microsoft::WRL::Make<Command>(std::move(entry)));
            children.push_back(Microsoft::WRL::Make<Command>(Function{L"", L"Manage functions\x2026", L"any"}));
            return Microsoft::WRL::Make<CommandEnum>(std::move(children)).CopyTo(output);
        } catch (...) { return E_FAIL; }
    }
};
class Factory final : public Microsoft::WRL::RuntimeClass<Microsoft::WRL::RuntimeClassFlags<Microsoft::WRL::ClassicCom>, IClassFactory> {
public:
    Factory() { ++objects; }
    ~Factory() { --objects; }
    IFACEMETHODIMP CreateInstance(IUnknown* outer, REFIID iid, void** output) override {
        if (!output) return E_POINTER;
        *output = nullptr;
        if (outer) return CLASS_E_NOAGGREGATION;
        auto command = Microsoft::WRL::Make<Command>();
        return command ? command->QueryInterface(iid, output) : E_OUTOFMEMORY;
    }
    IFACEMETHODIMP LockServer(BOOL lock) override { lock ? ++locks : --locks; return S_OK; }
};
extern "C" HRESULT __stdcall DllGetClassObject(REFCLSID clsid, REFIID iid, void** output) {
    if (!output) return E_POINTER;
    *output = nullptr;
    if (clsid != CLSID_Utilities) return CLASS_E_CLASSNOTAVAILABLE;
    auto factory = Microsoft::WRL::Make<Factory>();
    return factory ? factory->QueryInterface(iid, output) : E_OUTOFMEMORY;
}
extern "C" HRESULT __stdcall DllCanUnloadNow() { return objects == 0 && locks == 0 ? S_OK : S_FALSE; }
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) { module = instance; DisableThreadLibraryCalls(instance); }
    return TRUE;
}
