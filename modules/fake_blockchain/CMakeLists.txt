cmake_minimum_required(VERSION 3.14)
project(fake_blockchain_plugin LANGUAGES CXX)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found: build with nix (logos-module-builder)")
endif()

logos_module(
    NAME fake_blockchain
    SOURCES
        src/fake_blockchain_impl.h
        src/fake_blockchain_impl.cpp
)
