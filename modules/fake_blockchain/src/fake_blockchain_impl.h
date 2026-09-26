#pragma once

#include <atomic>
#include <condition_variable>
#include <cstdint>
#include <mutex>
#include <string>
#include <thread>

#include <logos_json.h>
#include <logos_module_context.h>
#include <logos_result.h>

// Answers the part of blockchain_module's contract the demo reads, with the
// same shapes (info as a JSON string in `value`), and emits newBlock each second.
class FakeBlockchainImpl : public LogosModuleContext {
public:
    ~FakeBlockchainImpl() override;

    StdLogosResult get_cryptarchia_info();
    StdLogosResult get_network_info();
    StdLogosResult get_time_info();
    StdLogosResult get_chain_id();
    StdLogosResult subscribe_to_new_blocks();
    // Who called, as the caller document this module was handed.
    LogosMap whoami();

logos_events:
    void newBlock(const std::string& blockJson);

protected:
    void onContextReady() override;
    LogosShutdown aboutToUnload() override;

private:
    void stopTicking();

    std::atomic<int64_t> m_height{0};
    std::mutex m_mutex;
    std::condition_variable m_wake;
    bool m_stopping = false;
    std::thread m_ticker;
};
