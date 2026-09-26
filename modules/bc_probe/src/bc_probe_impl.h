#pragma once

#include <logos_json.h>
#include <logos_module_context.h>

// A second module that reads the node through blockchain_module: the demo's
// module-to-module call, which crosses to the peered runtime when the node is
// an import.
class BcProbeImpl : public LogosModuleContext {
public:
    // {ok, info: {height, slot, tip, lib_slot, mode, ...}, latency_ms, error?}
    LogosMap chain_info_via_bc();
};
