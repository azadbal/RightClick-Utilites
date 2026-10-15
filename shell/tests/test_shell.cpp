#include "../UtilitiesShell.cpp"
#include <shellapi.h>
#include <iostream>
#include <stdexcept>

static void Check(bool value, const char* message) { if (!value) throw std::runtime_error(message); }
static ComPtr<IShellItemArray> Items(const std::vector<std::filesystem::path>& paths) {
    std::vector<PIDLIST_ABSOLUTE> ids;
    for (auto& path : paths) {
        PIDLIST_ABSOLUTE id = nullptr;
        Check(SUCCEEDED(SHParseDisplayName(path.c_str(), nullptr, &id, 0, nullptr)), "parse selection");
        ids.push_back(id);
    }
    ComPtr<IShellItemArray> items;
    HRESULT hr = SHCreateShellItemArrayFromIDLists(static_cast<UINT>(ids.size()), const_cast<PCIDLIST_ABSOLUTE*>(ids.data()), &items);
    for (auto id : ids) CoTaskMemFree(id);
    Check(SUCCEEDED(hr), "create selection");
    return items;
}
static EXPCMDSTATE State(IExplorerCommand* command, IShellItemArray* items) {
    EXPCMDSTATE state;
    Check(SUCCEEDED(command->GetState(items, FALSE, &state)), "get state");
    return state;
}
int wmain(int argc, wchar_t** argv) {
    if (argc != 2) return 2;
    HRESULT initialized = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(initialized)) return 2;
    auto temporary = std::filesystem::temp_directory_path() / (L"Utilities shell test " + std::to_wstring(GetCurrentProcessId()));
    try {
        std::filesystem::create_directory(temporary);
        auto video = temporary / L"sample unicode \x03a9 & spaces.MP4";
        auto text = temporary / L"sample.txt";
        auto png = temporary / L"image unicode \x03a9 & spaces.PNG";
        auto secondPng = temporary / L"second.png";
        auto folder = temporary / L"Folder with spaces";
        std::ofstream(video) << "fixture";
        std::ofstream(text) << "fixture";
        std::ofstream(png) << "fixture";
        std::ofstream(secondPng) << "fixture";
        std::filesystem::create_directory(folder);
        auto videos = Items({video});
        auto texts = Items({text});
        auto folders = Items({folder});
        auto multiple = Items({video,text});
        auto pngs = Items({png});
        auto multiplePngs = Items({png,secondPng});
        auto pngAndFolder = Items({png,folder});
        auto mixedPngs = Items({png,text});
        auto defaults = Defaults();
        auto command = Microsoft::WRL::Make<Command>(defaults[0]);
        Check(State(command.Get(), videos.Get()) == ECS_ENABLED, "video enabled");
        Check(State(command.Get(), texts.Get()) == ECS_HIDDEN, "extension filter");
        Check(State(command.Get(), folders.Get()) == ECS_HIDDEN, "folder filter");
        Check(State(command.Get(), multiple.Get()) == ECS_DISABLED, "multi selection disabled");
        Check(command->Invoke(multiple.Get(), nullptr) == E_INVALIDARG, "multi invoke rejected");
        Check(State(command.Get(), nullptr) == ECS_ENABLED, "enumeration without selection");
        auto pngCommand = Microsoft::WRL::Make<Command>(defaults[4]);
        Check(State(pngCommand.Get(), pngs.Get()) == ECS_ENABLED, "PNG enabled case insensitive");
        Check(State(pngCommand.Get(), multiplePngs.Get()) == ECS_ENABLED, "multiple PNGs enabled");
        Check(State(pngCommand.Get(), folders.Get()) == ECS_ENABLED, "PNG folder enabled");
        Check(State(pngCommand.Get(), pngAndFolder.Get()) == ECS_ENABLED, "PNG and folder enabled");
        Check(State(pngCommand.Get(), mixedPngs.Get()) == ECS_HIDDEN, "mixed PNG selection rejected");
        Check(State(pngCommand.Get(), videos.Get()) == ECS_HIDDEN, "PNG rejects video");
        auto folderCommand = Microsoft::WRL::Make<Command>(Function{L"folder", L"Folder command", L"folder", true, {L".mp4"}});
        Check(State(folderCommand.Get(), folders.Get()) == ECS_ENABLED, "folder command enabled");
        Check(State(folderCommand.Get(), videos.Get()) == ECS_HIDDEN, "folder command rejects file");
        auto manage = Microsoft::WRL::Make<Command>(Function{L"", L"Manage functions\x2026", L"any"});
        Check(State(manage.Get(), folders.Get()) == ECS_ENABLED, "manage folder enabled");
        Check(State(manage.Get(), videos.Get()) == ECS_ENABLED, "manage file enabled");
        Check(State(manage.Get(), multiple.Get()) == ECS_ENABLED, "manage multiple enabled");
        Selection selected;
        Check(GetSelection(folders.Get(), selected) == S_OK && selected.path == folder.wstring() && selected.folder, "actual folder filesystem path");
        Check(GetSelection(pngAndFolder.Get(), selected) == S_OK && selected.count == 2 && selected.items.size() == 2 &&
            selected.items[0].path == png.wstring() && !selected.items[0].folder &&
            selected.items[1].path == folder.wstring() && selected.items[1].folder, "all selected filesystem paths and types");
        Selection oversized;
        oversized.items.push_back({std::wstring(32767, L'x')});
        Check(Launch(L"", oversized) == HRESULT_FROM_WIN32(ERROR_FILENAME_EXCED_RANGE), "oversized command rejected before process launch");
        for (std::wstring value : {L"", L"plain", L"space and \x03a9", L"ends\\", L"a\\\"b & c", L"C:\\"}) {
            int count;
            PWSTR* parsed = CommandLineToArgvW((L"app " + Quote(value)).c_str(), &count);
            Check(parsed && count == 2 && parsed[1] == value, "Windows argv quoting roundtrip");
            LocalFree(parsed);
        }
        auto config = temporary / L"functions.json";
        Check(ReadFunctionsAt(config).size() == 5, "missing config defaults");
        std::ofstream(config) << R"({"version":1,"functions":[{"id":"custom","label":"Custom folder","enabled":true,"appliesTo":"folder","extensions":[]},{"id":"disabled","label":"Disabled","enabled":false,"appliesTo":"any"}]})";
        auto custom = ReadFunctionsAt(config);
        Check(custom.size() == 1 && custom[0].label == L"Custom folder", "configuration and disabled filtering");
        Check(!custom[0].supportsMultiple, "existing configuration defaults to single selection");
        std::ofstream(config) << R"({"version":1,"functions":[{"id":"png-jpeg","label":"PNG to JPEG","appliesTo":"any","extensions":[".PNG"],"supportsMultiple":true}]})";
        auto configuredPng = ReadFunctionsAt(config);
        Check(configuredPng.size() == 1 && configuredPng[0].supportsMultiple, "configuration supports multiple");
        Check(State(Microsoft::WRL::Make<Command>(configuredPng[0]).Get(), multiplePngs.Get()) == ECS_ENABLED, "configured PNG multiple enabled");
        std::ofstream(config) << R"({"version":1,"functions":[]})";
        Check(ReadFunctionsAt(config).empty(), "dynamic config update");
        std::ofstream(config) << "invalid json";
        Check(ReadFunctionsAt(config).empty(), "invalid config retains safe manage-only menu");

        auto copiedDll = temporary / L"UtilitiesShell.dll";
        auto probe = std::filesystem::path(ModuleDirectory()) / L"launch_probe.exe";
        std::filesystem::copy_file(argv[1], copiedDll);
        std::filesystem::copy_file(probe, temporary / L"Utilities.exe");
        auto log = temporary / L"launched-arguments.txt";
        SetEnvironmentVariableW(L"UTILITY_SHELL_TEST_LOG", log.c_str());
        HMODULE dll = LoadLibraryW(copiedDll.c_str());
        Check(dll != nullptr, "load built DLL");
        auto getClass = reinterpret_cast<HRESULT(__stdcall*)(REFCLSID,REFIID,void**)>(GetProcAddress(dll, "DllGetClassObject"));
        Check(getClass != nullptr, "export class factory");
        ComPtr<IClassFactory> factory;
        Check(SUCCEEDED(getClass(CLSID_Utilities, IID_PPV_ARGS(&factory))), "class factory");
        ComPtr<IExplorerCommand> root;
        Check(SUCCEEDED(factory->CreateInstance(nullptr, IID_PPV_ARGS(&root))), "create DLL command");
        PWSTR title = nullptr;
        Check(SUCCEEDED(root->GetTitle(videos.Get(), &title)) && std::wstring(title) == L"RightClick Utilities", "root title");
        CoTaskMemFree(title);
        Check(State(root.Get(), multiple.Get()) == ECS_ENABLED, "root multi enabled");
        ComPtr<IEnumExplorerCommand> enumeration;
        Check(SUCCEEDED(root->EnumSubCommands(&enumeration)), "enumerate DLL children");
        unsigned count = 0;
        std::wstring last;
        ComPtr<IExplorerCommand> child;
        ComPtr<IExplorerCommand> manageChild, functionChild;
        ULONG fetched;
        while (enumeration->Next(1, child.ReleaseAndGetAddressOf(), &fetched) == S_OK) {
            Check(SUCCEEDED(child->GetTitle(nullptr, &title)), "child title");
            last = title; CoTaskMemFree(title); ++count;
            if (last == L"Manage functions\x2026") manageChild = child;
            else if (!functionChild && State(child.Get(), videos.Get()) == ECS_ENABLED) functionChild = child;
        }
        Check(count == ReadFunctions().size() + 1 && last == L"Manage functions\x2026", "configured children plus manage");
        auto checkLaunch = [&](IExplorerCommand* launchCommand, IShellItemArray* selection, const std::string& expected) {
            std::error_code ignored;
            std::filesystem::remove(log, ignored);
            Check(SUCCEEDED(launchCommand->Invoke(selection, nullptr)), "DLL Invoke launches application");
            std::string content;
            for (int attempt = 0; attempt < 100; ++attempt) {
                std::ifstream input(log, std::ios::binary);
                content = std::string(std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>());
                if (content == expected) break;
                Sleep(50);
            }
            Check(content == expected, "actual launched argv matches selection");
        };
        checkLaunch(manageChild.Get(), folders.Get(), "--input\n" + winrt::to_string(folder.wstring()) + "\n");
        checkLaunch(manageChild.Get(), pngAndFolder.Get(), "--input\n" + winrt::to_string(png.wstring()) + "\n--input\n" + winrt::to_string(folder.wstring()) + "\n");
        if (functionChild) {
            PWSTR functionTitle;
            functionChild->GetTitle(nullptr, &functionTitle);
            std::wstring label = functionTitle; CoTaskMemFree(functionTitle);
            auto functions = ReadFunctions();
            auto selectedFunction = std::find_if(functions.begin(), functions.end(), [&](auto& entry) { return entry.label == label; });
            Check(selectedFunction != functions.end(), "selected configured function");
            checkLaunch(functionChild.Get(), videos.Get(), "--run-function\n" + winrt::to_string(selectedFunction->id) + "\n--input\n" + winrt::to_string(video.wstring()) + "\n");
        }
        SetEnvironmentVariableW(L"UTILITY_SHELL_TEST_LOG", nullptr);
        Check(SUCCEEDED(enumeration->Reset()), "enum reset");
        ComPtr<IEnumExplorerCommand> cloned;
        Check(SUCCEEDED(enumeration->Clone(&cloned)), "enum clone");
        Check(enumeration->Skip(count + 1) == S_FALSE, "enum skip past end");
        child.Reset(); manageChild.Reset(); functionChild.Reset(); cloned.Reset(); enumeration.Reset(); root.Reset(); factory.Reset();
        auto unload = reinterpret_cast<HRESULT(__stdcall*)()>(GetProcAddress(dll,"DllCanUnloadNow"));
        Check(unload && unload() == S_OK, "DLL COM lifetime");
        FreeLibrary(dll);
        std::filesystem::remove_all(temporary);
        std::cout << "PASS: DLL activation, enumeration, config updates, PNG multiselect filtering, command length limit, Unicode quoting, actual repeated Invoke argv, COM lifetime\n";
    } catch (const std::exception& error) {
        std::cerr << "FAIL: " << error.what() << "\n";
        std::error_code ignored;
        std::filesystem::remove_all(temporary, ignored);
        CoUninitialize(); return 1;
    }
    CoUninitialize();
    return 0;
}
