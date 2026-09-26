#include "bc_probe_impl.h"

#include "logos_sdk.h"

#include <chrono>

LogosMap BcProbeImpl::chain_info_via_bc()
{
    const auto started = std::chrono::steady_clock::now();
    logos::CallError error;
    const StdLogosResult reply = modules().blockchain_module.get_cryptarchia_info(&error);
    const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - started).count();

    LogosMap out = {{"latency_ms", static_cast<int64_t>(ms)}};
    if (!error.ok() || !reply.success) {
        out["ok"] = false;
        out["error"] = !error.ok() ? error.code + ": " + error.message : reply.error;
        return out;
    }
    // The node answers its info as a JSON document in a string.
    const LogosMap info = reply.value.is_string()
        ? LogosMap::parse(reply.value.get<std::string>(), nullptr, false)
        : reply.value;
    out["ok"] = !info.is_discarded();
    out["info"] = info.is_discarded() ? LogosMap() : info;
    return out;
}
