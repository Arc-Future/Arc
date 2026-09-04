namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.CommandLine;
using Arc.QIF;

/// <summary>
/// Arc.CommandLine Stable 最小面（非 Fact-Skip；非 L3 DI）。
/// Option/Argument 契约、Parse（含选项匹配）、IConsole PrintHelp、帮助快路径。
/// 完整 System.CommandLine 对等后置。
/// </summary>
public class CommandLineTests
{
    [Fact]
    public void Option_FlagAndAliases()
    {
        Option verbose = new Option("--verbose", "-v", "verbose");
        verbose.IsFlag = true;
        Assert.True(verbose.IsFlag);
        Assert.True(verbose.MatchesAlias("-v"));
        Assert.True(verbose.MatchesAlias("--verbose"));
        Assert.Equal("--verbose", verbose.PrimaryAlias);
        Assert.Contains("-v", verbose.ToAliasString());
    }

    [Fact]
    public void Option_FromAmong_RejectsUnknown()
    {
        Option mode = new Option("--mode", "mode");
        List<string> allowed = new List<string>();
        allowed.Add("debug");
        allowed.Add("release");
        mode.FromAmong(allowed);
        Assert.Equal("", mode.Validate("debug"));
        Assert.True(mode.Validate("ship").Length > 0);
    }

    [Fact]
    public void Argument_RequiredDefaults()
    {
        Argument fileArg = new Argument("file", "input");
        fileArg.IsRequired = true;
        fileArg.DefaultValue = "in.as";
        Assert.Equal("file", fileArg.Name);
        Assert.True(fileArg.IsRequired);
        Assert.Equal("in.as", fileArg.DefaultValue);
    }

    [Fact]
    public void Parse_Empty_MissingRequired_HasError()
    {
        RootCommand root = new RootCommand("cli");
        Option cfg = new Option("--config", "config path");
        cfg.IsRequired = true;
        root.AddOption(cfg);
        List<string> tokens = new List<string>();
        ParseResult result = root.Parse(tokens);
        Assert.True(result.HasErrors);
    }

    [Fact]
    public void Parse_DefaultValueWhenOptionOmitted()
    {
        RootCommand root = new RootCommand("cli");
        Option port = new Option("--port", "port");
        port.DefaultValue = "8080";
        root.AddOption(port);
        List<string> tokens = new List<string>();
        ParseResult result = root.Parse(tokens);
        Assert.True(!result.HasErrors);
        Assert.Equal("8080", result.GetValueForOption(port));
        Assert.True(!result.HasOption(port));
    }

    [Fact]
    public void Parse_Help_FastPath()
    {
        RootCommand root = new RootCommand("help app");
        List<string> tokens = new List<string>();
        tokens.Add("--help");
        ParseResult result = root.Parse(tokens);
        Assert.True(result.IsHelp);
    }

    [Fact]
    public void RootCommand_RegistersSubcommand()
    {
        RootCommand root = new RootCommand("cli");
        Command build = new Command("build", "build project");
        root.AddCommand(build);
        Assert.Equal("build", build.Name);
        Assert.Equal("build project", build.Description);
    }

    [Fact]
    public void CaptureConsole_ConcreteWrite()
    {
        CaptureConsole console = new CaptureConsole();
        console.WriteLine("hello");
        Assert.Contains("hello", console.OutText);
    }

    [Fact]
    public void Parse_FlagOption_Matched()
    {
        RootCommand root = new RootCommand("cli");
        Option verbose = new Option("--verbose", "-v", "verbose");
        verbose.IsFlag = true;
        root.AddOption(verbose);
        List<string> tokens = new List<string>();
        tokens.Add("--verbose");
        ParseResult result = root.Parse(tokens);
        Assert.True(!result.HasErrors);
        Assert.True(result.HasOption(verbose));
    }

    [Fact]
    public void PrintHelp_ViaIConsole()
    {
        RootCommand root = new RootCommand("help app");
        CaptureConsole console = new CaptureConsole();
        IConsole ic = console;
        root.PrintHelp(ic);
        Assert.True(console.OutText.Length > 0);
        Assert.Contains("Usage:", console.OutText);
    }
}
