cmake_minimum_required(VERSION 3.16)
project(kakangkung_ladybird_host LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

option(HAVE_LADYBIRD "Link against a Ladybird / LibWebView build" OFF)
set(LADYBIRD_SOURCE_DIR "" CACHE PATH "Path to a Ladybird checkout (required when HAVE_LADYBIRD=ON)")
set(LADYBIRD_BUILD_DIR "" CACHE PATH "Path to a Ladybird build directory (required when HAVE_LADYBIRD=ON)")

set(HOST_SOURCES
    src/host_main.cpp
    src/application.cpp
    src/abi.cpp
)
if(HAVE_LADYBIRD)
    list(APPEND HOST_SOURCES src/view_ladybird.cpp)
else()
    list(APPEND HOST_SOURCES src/stub_page.cpp)
endif()

add_executable(kakangkung-ladybird-host ${HOST_SOURCES})

target_include_directories(kakangkung-ladybird-host PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)

target_compile_definitions(kakangkung-ladybird-host PRIVATE KAKANGKUNG_LADYBIRD_BUILD)

if(HAVE_LADYBIRD)
    if(NOT LADYBIRD_SOURCE_DIR OR NOT LADYBIRD_BUILD_DIR)
        message(FATAL_ERROR "HAVE_LADYBIRD=ON requires LADYBIRD_SOURCE_DIR and LADYBIRD_BUILD_DIR")
    endif()
    target_compile_definitions(kakangkung-ladybird-host PRIVATE HAVE_LADYBIRD=1)
    target_include_directories(kakangkung-ladybird-host PRIVATE
        ${LADYBIRD_SOURCE_DIR}
        ${LADYBIRD_SOURCE_DIR}/Userland/Libraries
        ${LADYBIRD_BUILD_DIR}
    )
    if(DEFINED LADYBIRD_LIBRARIES)
        target_link_libraries(kakangkung-ladybird-host PRIVATE ${LADYBIRD_LIBRARIES})
    else()
        message(WARNING "HAVE_LADYBIRD=ON but LADYBIRD_LIBRARIES is empty; finish view_ladybird.cpp + LibWebView link on Boat.")
    endif()
endif()

if(WIN32)
    target_link_libraries(kakangkung-ladybird-host PRIVATE user32 gdi32)
endif()

install(TARGETS kakangkung-ladybird-host RUNTIME DESTINATION bin)
