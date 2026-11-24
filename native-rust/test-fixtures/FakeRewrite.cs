// Compile as codex.exe only in an isolated test directory. This executable never uses the network.
using System;
using System.IO;
using System.Linq;
class FakeRewrite {
    static int Main(string[] args) {
        if(args.Contains("login") && args.Contains("status")) {Console.Error.WriteLine("Logged in using ChatGPT");return 0;}
        if(args.Contains("--bundled")) {Console.WriteLine("{\"models\":[{\"slug\":\"test-model\"}]}");return 0;}
        string home=Environment.GetEnvironmentVariable("CODEX_HOME");
        if(args.FirstOrDefault()!="exec" || !args.Contains("read-only") || !args.Contains("--ephemeral") ||
           !args.Contains("--ignore-user-config") || !args.Contains("test-model") || !args.Contains("features.shell_tool=false") ||
           !args.Contains("web_search=\"disabled\"") || !args.Contains("cli_auth_credentials_store=\"file\"") ||
           Path.GetDirectoryName(Environment.CurrentDirectory)!=home || Path.GetFileName(home)!="codex-rewrite") return 3;
        Console.InputEncoding=new System.Text.UTF8Encoding(false);
        if(!Console.In.ReadToEnd().Contains("synthetic-wording"))return 4;
        Console.WriteLine("Verified rewritten test text.");return 0;
    }
}
