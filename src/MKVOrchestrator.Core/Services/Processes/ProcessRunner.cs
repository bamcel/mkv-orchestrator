using System.ComponentModel;
using System.Diagnostics;
using System.Text;

namespace MKVOrchestrator.Core.Services;

public sealed record ProcessResult(int ExitCode, string StandardOutput, string StandardError);

public sealed class ProcessRunner
{
    public async Task<ProcessResult> RunAsync(string exePath, IEnumerable<string> args, CancellationToken token)
    {
        if (string.IsNullOrWhiteSpace(exePath))
            throw new InvalidOperationException("Executable path is blank.");

        var psi = BuildStartInfo(exePath, args);

        try
        {
            using var process = new Process { StartInfo = psi, EnableRaisingEvents = true };
            process.Start();

            // WaitForExitAsync only stops waiting on cancellation; the external tool keeps
            // running unless it is killed explicitly, so canceled scans must tear it down.
            using var registration = token.Register(() =>
            {
                try
                {
                    if (!process.HasExited)
                        process.Kill(entireProcessTree: true);
                }
                catch
                {
                    // Best-effort cancellation cleanup.
                }
            });

            var stdoutTask = process.StandardOutput.ReadToEndAsync(token);
            var stderrTask = process.StandardError.ReadToEndAsync(token);
            await process.WaitForExitAsync(token);
            return new ProcessResult(process.ExitCode, await stdoutTask, await stderrTask);
        }
        catch (Win32Exception ex)
        {
            throw new InvalidOperationException($"Could not start '{exePath}'. Check the path in Settings. {ex.Message}", ex);
        }
    }

    public async Task<ProcessResult> RunWithOutputCallbackAsync(
        string exePath,
        IEnumerable<string> args,
        Action<string>? onOutputLine,
        CancellationToken token)
    {
        if (string.IsNullOrWhiteSpace(exePath))
            throw new InvalidOperationException("Executable path is blank.");

        var psi = BuildStartInfo(exePath, args);
        var stdout = new StringBuilder();
        var stderr = new StringBuilder();

        try
        {
            using var process = new Process { StartInfo = psi, EnableRaisingEvents = true };
            process.OutputDataReceived += (_, e) =>
            {
                if (e.Data is null) return;
                lock (stdout) stdout.AppendLine(e.Data);
                onOutputLine?.Invoke(e.Data);
            };
            process.ErrorDataReceived += (_, e) =>
            {
                if (e.Data is null) return;
                lock (stderr) stderr.AppendLine(e.Data);
                onOutputLine?.Invoke(e.Data);
            };

            process.Start();
            process.BeginOutputReadLine();
            process.BeginErrorReadLine();

            using var registration = token.Register(() =>
            {
                try
                {
                    if (!process.HasExited)
                        process.Kill(entireProcessTree: true);
                }
                catch
                {
                    // Best-effort cancellation cleanup.
                }
            });

            await process.WaitForExitAsync(token);
            return new ProcessResult(process.ExitCode, stdout.ToString(), stderr.ToString());
        }
        catch (Win32Exception ex)
        {
            throw new InvalidOperationException($"Could not start '{exePath}'. Check the path in Settings. {ex.Message}", ex);
        }
    }

    private static ProcessStartInfo BuildStartInfo(string exePath, IEnumerable<string> args)
    {
        var psi = new ProcessStartInfo
        {
            FileName = CrossPlatformRuntime.NormalizeUserPath(exePath),
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8
        };
        foreach (var arg in args) psi.ArgumentList.Add(arg);
        return psi;
    }
}
