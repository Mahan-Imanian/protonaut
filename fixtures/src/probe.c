/* fx_probe: behaves like a hostile game build to test the action's sandbox.
 * Each probe writes one line, "PROBE <name>: BLOCKED|OPEN|none|found ...",
 * to probe-results.txt next to the executable (Proton does not pass a GUI
 * program's stderr through). It never records file contents or secret
 * values, only whether access succeeded. See docs/security. */
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include <stdio.h>
#include <string.h>

static FILE *out;

static void line(const char *text) {
    if (out) {
        fputs(text, out);
        fflush(out);
    }
}

static void report(const char *name, int open, const char *detail) {
    char buf[256];
    snprintf(buf, sizeof buf, "PROBE %s: %s%s%s\n", name, open ? "OPEN" : "BLOCKED", detail[0] ? " " : "", detail);
    line(buf);
}

static void probe_network(void) {
    WSADATA wsa;
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) {
        report("net_tcp", 0, "(winsock unavailable)");
        return;
    }
    struct addrinfo *res = NULL;
    report("dns", getaddrinfo("github.com", "443", NULL, &res) == 0, "github.com");
    if (res) freeaddrinfo(res);

    SOCKET s = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    struct sockaddr_in a = {0};
    a.sin_family = AF_INET;
    a.sin_port = htons(443);
    a.sin_addr.s_addr = inet_addr("1.1.1.1");
    u_long nb = 1;
    ioctlsocket(s, FIONBIO, &nb);
    connect(s, (struct sockaddr *)&a, sizeof a);
    fd_set w;
    FD_ZERO(&w);
    FD_SET(s, &w);
    struct timeval tv = {5, 0};
    int ok = 0;
    if (select(0, NULL, &w, NULL, &tv) == 1) {
        int err = 0;
        int len = sizeof err;
        getsockopt(s, SOL_SOCKET, SO_ERROR, (char *)&err, &len);
        ok = (err == 0);
    }
    report("net_tcp", ok, "1.1.1.1:443");
    closesocket(s);
    WSACleanup();
}

/* Can the directory be listed? */
static void probe_dir(const char *name, const char *dir) {
    char pattern[MAX_PATH];
    snprintf(pattern, sizeof pattern, "%s\\*", dir);
    WIN32_FIND_DATAA fd;
    HANDLE h = FindFirstFileA(pattern, &fd);
    int entries = 0;
    if (h != INVALID_HANDLE_VALUE) {
        do {
            if (strcmp(fd.cFileName, ".") && strcmp(fd.cFileName, "..")) entries++;
        } while (FindNextFileA(h, &fd));
        FindClose(h);
    }
    char detail[64];
    snprintf(detail, sizeof detail, "(%d entries)", entries);
    report(name, entries > 0, detail);
}

/* Reads every /proc/<pid>/environ it can and looks for CI token variable
 * names. Reports counts only. */
static void probe_proc_tokens(void) {
    WIN32_FIND_DATAA fd;
    HANDLE h = FindFirstFileA("Z:\\proc\\*", &fd);
    int readable = 0, with_token = 0;
    static char buf[1 << 16];
    if (h != INVALID_HANDLE_VALUE) {
        do {
            if (fd.cFileName[0] < '0' || fd.cFileName[0] > '9') continue;
            char path[MAX_PATH];
            snprintf(path, sizeof path, "Z:\\proc\\%s\\environ", fd.cFileName);
            HANDLE f = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
            if (f == INVALID_HANDLE_VALUE) continue;
            DWORD n = 0;
            if (ReadFile(f, buf, sizeof buf - 1, &n, NULL) && n > 0) {
                readable++;
                for (DWORD i = 0; i < n; i++)
                    if (buf[i] == 0) buf[i] = '\n';
                buf[n] = 0;
                if (strstr(buf, "GITHUB_TOKEN=") || strstr(buf, "ACTIONS_RUNTIME_TOKEN=") || strstr(buf, "ACTIONS_ID_TOKEN_REQUEST_TOKEN="))
                    with_token++;
            }
            CloseHandle(f);
        } while (FindNextFileA(h, &fd));
        FindClose(h);
    }
    char msg[128];
    snprintf(msg, sizeof msg, "PROBE proc_tokens: %s (%d environ files readable)\n", with_token ? "found" : "none", readable);
    line(msg);
}

static void probe_write(const char *name, const char *path) {
    HANDLE f = CreateFileA(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, 0, NULL);
    int ok = f != INVALID_HANDLE_VALUE;
    if (ok) {
        CloseHandle(f);
        DeleteFileA(path);
    }
    report(name, ok, "");
}

/* Runs a Linux command line through Wine's start.exe and returns its exit
 * code, or -1 when it could not be started. */
static long run_unix(const char *cmdline) {
    char cmd[512];
    snprintf(cmd, sizeof cmd, "start.exe /wait /unix %s", cmdline);
    STARTUPINFOA si = {sizeof si};
    PROCESS_INFORMATION pi;
    if (!CreateProcessA(NULL, cmd, NULL, NULL, FALSE, CREATE_NO_WINDOW, NULL, NULL, &si, &pi)) return -1;
    WaitForSingleObject(pi.hProcess, 15000);
    DWORD code = (DWORD)-1;
    GetExitCodeProcess(pi.hProcess, &code);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    return (long)code;
}

int WINAPI WinMain(HINSTANCE inst, HINSTANCE prev, LPSTR cmd, int show) {
    (void)inst; (void)prev; (void)cmd; (void)show;
    char path[MAX_PATH];
    DWORD n = GetModuleFileNameA(NULL, path, MAX_PATH);
    while (n > 0 && path[n - 1] != '\\') n--;
    path[n] = 0;
    strncat(path, "probe-results.txt", MAX_PATH - n - 1);
    out = fopen(path, "w");
    probe_network();
    probe_dir("runner_work", "Z:\\home\\runner\\work");
    probe_dir("runner_temp", "Z:\\home\\runner\\work\\_temp");
    probe_dir("runner_home", "Z:\\home\\runner");
    probe_proc_tokens();
    probe_write("write_runner_home", "Z:\\home\\runner\\preflight-probe.txt");
    /* start.exe does not pass a Unix program's exit code back, so sudo
     * proves itself by creating a marker file. */
    DeleteFileA("Z:\\tmp\\pfprobe_sudo");
    run_unix("/bin/sh -c \"/usr/bin/sudo -n /usr/bin/true && /usr/bin/touch /tmp/pfprobe_sudo\"");
    int escalated = 0;
    for (int i = 0; i < 20 && !escalated; i++) {
        escalated = GetFileAttributesA("Z:\\tmp\\pfprobe_sudo") != INVALID_FILE_ATTRIBUTES;
        if (!escalated) Sleep(500);
    }
    DeleteFileA("Z:\\tmp\\pfprobe_sudo");
    report("sudo", escalated, "(sudo -n as the game's user)");
    /* Leave a process outside the game's process group and Wine prefix;
     * the action must still stop it before collecting results. */
    run_unix("/usr/bin/setsid -f /usr/bin/sleep 900");
    line("PROBE escape: spawned a detached sleep 900 outside the process group\n");
    line("PROBE done\n");
    Sleep(600000);
    return 0;
}
