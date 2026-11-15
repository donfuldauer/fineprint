// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
// If a copy of the MPL was not distributed with this file, You can obtain one at
// https://mozilla.org/MPL/2.0/.

// FP-005 (#6) spike (REQ-NFR-002, REQ-NFR-003): P/Invoke wrapper over the spike C ABI
// (include/fineprint_spike.h) using the LibraryImport source generator.

using System;
using System.Runtime.InteropServices;

namespace FinePrint.Spike.Interop;

/// <summary>Managed access to the spike's <c>fp_spike_sha256</c> C function.</summary>
public static partial class SpikeNative
{
    /// <summary>
    /// Library name without prefix or extension; the runtime probes
    /// <c>libfineprint_spike_bindings.so</c>, <c>fineprint_spike_bindings.dll</c> or
    /// <c>libfineprint_spike_bindings.dylib</c> in the RID-specific native asset directory.
    /// </summary>
    public const string LibraryName = "fineprint_spike_bindings";

    /// <summary>Length in bytes of a SHA-256 digest (<c>FP_SPIKE_SHA256_LEN</c>).</summary>
    public const int Sha256Length = 32;

    /// <summary>Success return code (<c>FP_SPIKE_OK</c>).</summary>
    public const int Ok = 0;

    [LibraryImport(LibraryName, EntryPoint = "fp_spike_sha256")]
    private static unsafe partial int FpSpikeSha256(byte* data, nuint len, byte* output);

    /// <summary>Calls the C function with raw arguments and returns its return code.</summary>
    /// <param name="data">Input bytes; an empty span passes a null pointer with length 0.</param>
    /// <param name="output">Destination; an empty span passes a null pointer.</param>
    /// <param name="lengthOverride">If set, passed as <c>len</c> instead of the span length
    /// (used only to exercise the error paths with a null <paramref name="data"/>).</param>
    /// <returns>0 on success, nonzero on bad arguments.</returns>
    public static unsafe int RawSha256(ReadOnlySpan<byte> data, Span<byte> output, nuint? lengthOverride = null)
    {
        if (lengthOverride is not null && !data.IsEmpty)
        {
            throw new ArgumentException("lengthOverride is only allowed with empty data", nameof(lengthOverride));
        }
        if (!output.IsEmpty && output.Length < Sha256Length)
        {
            throw new ArgumentException("output must hold 32 bytes", nameof(output));
        }
        fixed (byte* d = data)
        fixed (byte* o = output)
        {
            return FpSpikeSha256(d, lengthOverride ?? (nuint)data.Length, o);
        }
    }

    /// <summary>Returns the SHA-256 digest of <paramref name="data"/>.</summary>
    /// <exception cref="InvalidOperationException">The native function returned an error code.</exception>
    public static byte[] Sha256(ReadOnlySpan<byte> data)
    {
        var output = new byte[Sha256Length];
        int rc = RawSha256(data, output);
        if (rc != Ok)
        {
            throw new InvalidOperationException($"fp_spike_sha256 returned {rc}");
        }
        return output;
    }

    /// <summary>Returns the SHA-256 digest of <paramref name="data"/> as lowercase hex.</summary>
    public static string Sha256Hex(ReadOnlySpan<byte> data) => Convert.ToHexStringLower(Sha256(data));
}
