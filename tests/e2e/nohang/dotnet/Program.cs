// SPDX-License-Identifier: MIT OR Apache-2.0
// No-hang checks for .NET: every wait below has a 1 s timeout and must return
// after about 1 s of real time while virtual time is far ahead. Each wait runs
// on its own thread and is measured with Stopwatch (monotonic).
// Prints one JSON line per primitive: {"test", "elapsed_ms", "status"}.
using System.Collections.Concurrent;
using System.Diagnostics;

if (args.Length > 0 && args[0] == "now")
{
    Console.WriteLine($"{{\"now\":{DateTimeOffset.UtcNow.ToUnixTimeMilliseconds() / 1000.0:F3}}}");
    return;
}

var held = new object();
var holder = new Thread(() => { lock (held) { Thread.Sleep(Timeout.Infinite); } }) { IsBackground = true };
holder.Start();
var heldMutex = new Mutex(false);
var mutexHolder = new Thread(() => { heldMutex.WaitOne(); Thread.Sleep(Timeout.Infinite); }) { IsBackground = true };
mutexHolder.Start();
Thread.Sleep(100);

var tests = new (string Name, Action Run)[]
{
    ("Thread.Sleep", () => Thread.Sleep(1000)),
    ("Task.Delay", () => Task.Delay(1000).Wait()),
    ("ManualResetEventSlim.Wait", () => new ManualResetEventSlim(false).Wait(1000)),
    ("ManualResetEvent.WaitOne", () => new ManualResetEvent(false).WaitOne(1000)),
    ("Monitor.TryEnter", () => Monitor.TryEnter(held, 1000)),
    ("Monitor.Wait", () => { var o = new object(); lock (o) { Monitor.Wait(o, 1000); } }),
    ("SemaphoreSlim.Wait", () => new SemaphoreSlim(0).Wait(1000)),
    ("Mutex.WaitOne", () => heldMutex.WaitOne(1000)),
    ("BlockingCollection.TryTake", () => new BlockingCollection<int>().TryTake(out _, 1000)),
    ("Timer", () => { using var e = new ManualResetEventSlim(false); using var t = new Timer(_ => e.Set(), null, 1000, Timeout.Infinite); e.Wait(10000); }),
};

foreach (var (name, run) in tests)
{
    var done = new ManualResetEventSlim(false);
    var sw = Stopwatch.StartNew();
    new Thread(() => { run(); done.Set(); }) { IsBackground = true }.Start();
    var finished = done.Wait(6000);
    var ms = sw.Elapsed.TotalMilliseconds;
    var status = !finished ? "HANG" : ms < 900 ? "EARLY" : ms > 2500 ? "SLOW" : "OK";
    Console.WriteLine($"{{\"test\":\"{name}\",\"elapsed_ms\":{Math.Round(ms)},\"status\":\"{status}\"}}");
    Console.Out.Flush();
}
Environment.Exit(0);
