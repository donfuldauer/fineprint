// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike (REQ-NFR-002): proves the native library sits at runtimes/<rid>/native/<lib>
// in the packed .nupkg and in the published app, and that the app's deps.json declares it as a
// native asset for that RID. Run as a .NET 10 file-based app:
//   dotnet run --file dotnet/check-rid-layout.cs -- <nupkg> <publish-dir> <rid> <lib>
// PublishAot is off so no ILCompiler package is restored for this helper.

#:property PublishAot=false

using System;
using System.IO;
using System.IO.Compression;
using System.Linq;

if (args.Length != 4)
{
    Console.Error.WriteLine("usage: check-rid-layout <nupkg> <publish-dir> <rid> <lib>");
    return 2;
}
string nupkg = args[0];
string publishDir = args[1];
string rid = args[2];
string lib = args[3];
string expected = $"runtimes/{rid}/native/{lib}";
int failures = 0;

using (ZipArchive zip = ZipFile.OpenRead(nupkg))
{
    string[] native = zip.Entries
        .Select(e => e.FullName)
        .Where(n => n.StartsWith("runtimes/", StringComparison.Ordinal))
        .OrderBy(n => n, StringComparer.Ordinal)
        .ToArray();
    Console.WriteLine($"{Path.GetFileName(nupkg)} runtime assets: {string.Join(", ", native)}");
    if (!native.Contains(expected))
    {
        Console.Error.WriteLine($"FAIL: {expected} not in {nupkg}");
        failures++;
    }
}

string published = Path.Combine(publishDir, "runtimes", rid, "native", lib);
if (File.Exists(published))
{
    Console.WriteLine($"published: {published}");
}
else
{
    Console.Error.WriteLine($"FAIL: {published} missing");
    failures++;
}

string[] depsFiles = Directory.GetFiles(publishDir, "*.deps.json");
if (depsFiles.Length != 1 || !File.ReadAllText(depsFiles[0]).Contains($"\"{expected}\"", StringComparison.Ordinal))
{
    Console.Error.WriteLine($"FAIL: deps.json in {publishDir} does not declare {expected}");
    failures++;
}

if (failures == 0)
{
    Console.WriteLine($"OK: {expected}");
}
return failures == 0 ? 0 : 1;
