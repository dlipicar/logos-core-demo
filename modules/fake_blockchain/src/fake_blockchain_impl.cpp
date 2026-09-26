#include "fake_blockchain_impl.h"

#include <logos_caller.h>

#include <chrono>

namespace {

constexpr int64_t kGenesisMs = 1767225600000; // 2026-01-01T00:00:00Z

int64_t nowMs()
{
    return std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
}

StdLogosResult okJson(const LogosMap& value)
{
    return StdLogosResult{true, value.dump(), ""};
}

std::string hexOf(int64_t height)
{
    static const char digits[] = "0123456789abcdef";
    std::string hex(64, '0');
    for (int i = 63; i >= 0 && height > 0; --i, height >>= 4) hex[i] = digits[height & 0xf];
    return hex;
}

} // namespace

FakeBlockchainImpl::~FakeBlockchainImpl()
{
    stopTicking();
}

void FakeBlockchainImpl::onContextReady()
{
    m_ticker = std::thread([this] {
        std::unique_lock<std::mutex> lock(m_mutex);
        while (!m_wake.wait_for(lock, std::chrono::seconds(1), [this] { return m_stopping; })) {
            const int64_t height = ++m_height;
            lock.unlock();
            // The real node's shape: a header with its slot and parent, no height.
            const LogosMap header = {{"parent_block", hexOf(height - 1)},
                                     {"slot", (nowMs() - kGenesisMs) / 1000}};
            newBlock(LogosMap{{"block", LogosMap{{"header", header}}.dump()}}.dump());
            lock.lock();
        }
    });
}

LogosShutdown FakeBlockchainImpl::aboutToUnload()
{
    stopTicking();
    return LogosShutdown::Synchronous;
}

void FakeBlockchainImpl::stopTicking()
{
    {
        std::lock_guard<std::mutex> lock(m_mutex);
        m_stopping = true;
    }
    m_wake.notify_all();
    if (m_ticker.joinable()) m_ticker.join();
}

StdLogosResult FakeBlockchainImpl::get_cryptarchia_info()
{
    const int64_t height = m_height.load();
    const int64_t slot = (nowMs() - kGenesisMs) / 1000;
    return okJson({{"lib", hexOf(0)}, {"lib_slot", 0}, {"tip", hexOf(height)}, {"slot", slot},
                   {"height", height}, {"mode", "Online"}});
}

StdLogosResult FakeBlockchainImpl::get_network_info()
{
    return okJson({{"n_peers", 3}, {"n_connections", 3}, {"n_pending_connections", 0},
                   {"n_discovered_peers", 5}});
}

StdLogosResult FakeBlockchainImpl::get_time_info()
{
    const int64_t now = nowMs();
    const int64_t slot = (now - kGenesisMs) / 1000;
    return okJson({{"slot_duration_ms", 1000}, {"genesis_time_unix_ms", kGenesisMs},
                   {"current_slot", slot}, {"current_epoch", slot / 21600}});
}

StdLogosResult FakeBlockchainImpl::get_chain_id()
{
    return StdLogosResult{true, "fake-devnet", ""};
}

StdLogosResult FakeBlockchainImpl::subscribe_to_new_blocks()
{
    return StdLogosResult{true, nullptr, ""};
}

LogosMap FakeBlockchainImpl::whoami()
{
    const logos::LogosCaller& caller = logos::currentCaller();
    return {{"remote", caller.isRemote()}, {"module", caller.isModule()},
            {"name", caller.name}, {"peer", caller.peer}};
}
