#pragma once

#include <atomic>
#include <cstdint>
#include <string>

#include <logos_module_context.h>

// The demo's local module: runs in the app's own runtime.
class HelloModuleImpl : public LogosModuleContext {
public:
    std::string ping();
    std::string echo(const std::string& text);
    // Emits `fired(tag, count)` and returns the count so far.
    int64_t fire(const std::string& tag);

logos_events:
    void fired(const std::string& tag, int64_t count);

private:
    std::atomic<int64_t> m_fired{0};
};
