// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike .NET caller (REQ-NFR-002, REQ-NFR-003).
// Usage: FinePrint.Spike.Console <vectors.txt>
// Hashes every input of the shared vectors file through P/Invoke and writes "<name> <hex>\n"
// per input (LF on every OS, so outputs are byte-identical). Exits nonzero on any API error,
// malformed input or digest mismatch.

using System;
using System.IO;
using System.Text;
using FinePrint.Spike.Interop;

if (args.Length != 1)
{
    Console.Error.WriteLine("usage: FinePrint.Spike.Console <vectors.txt>");
    return 2;
}

// The API must reject bad arguments with a nonzero code (REQ-NFR-002).
if (SpikeNative.RawSha256("abc"u8, Span<byte>.Empty) == SpikeNative.Ok)
{
    Console.Error.WriteLine("null out was accepted");
    return 1;
}
if (SpikeNative.RawSha256(ReadOnlySpan<byte>.Empty, new byte[SpikeNative.Sha256Length], lengthOverride: 1) == SpikeNative.Ok)
{
    Console.Error.WriteLine("null data with len 1 was accepted");
    return 1;
}

var output = new StringBuilder();
int count = 0;
foreach (string rawLine in File.ReadAllText(args[0], Encoding.UTF8).Split('\n'))
{
    string[] fields = rawLine.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
    if (fields.Length == 0)
    {
        continue;
    }
    if (fields.Length != 3)
    {
        Console.Error.WriteLine($"malformed line: {rawLine}");
        return 1;
    }
    byte[] input = fields[1] == "-" ? Array.Empty<byte>() : Convert.FromHexString(fields[1]);
    string hex = SpikeNative.Sha256Hex(input);
    if (hex != fields[2])
    {
        Console.Error.WriteLine($"digest mismatch for {fields[0]}: got {hex} want {fields[2]}");
        return 1;
    }
    output.Append(fields[0]).Append(' ').Append(hex).Append('\n');
    count++;
}
if (count == 0)
{
    Console.Error.WriteLine("no vectors");
    return 1;
}

// Write raw UTF-8 bytes so the newline is LF on every OS.
using Stream stdout = Console.OpenStandardOutput();
byte[] bytes = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false).GetBytes(output.ToString());
stdout.Write(bytes, 0, bytes.Length);
stdout.Flush();
return 0;
