#include "hello_module_impl.h"

std::string HelloModuleImpl::ping()
{
    return "pong";
}

std::string HelloModuleImpl::echo(const std::string& text)
{
    return text;
}

int64_t HelloModuleImpl::fire(const std::string& tag)
{
    const int64_t count = ++m_fired;
    fired(tag, count);
    return count;
}
