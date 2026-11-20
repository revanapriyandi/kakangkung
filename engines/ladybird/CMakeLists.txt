cmake_minimum_required(VERSION 3.25)
project(kakangkung_ladybird_host LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 23)
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
        ${LADYBIRD_SOURCE_DIR}/AK
        ${LADYBIRD_SOURCE_DIR}/Libraries
        ${LADYBIRD_SOURCE_DIR}/Userland/Libraries
        ${LADYBIRD_BUILD_DIR}
        ${LADYBIRD_BUILD_DIR}/AK
        ${LADYBIRD_BUILD_DIR}/Libraries
    )
    # Discover Ladybird static/shared libs from the build tree.
    file(GLOB_RECURSE _LB_WEBVIEWS
        "${LADYBIRD_BUILD_DIR}/**/libwebview*.a"
        "${LADYBIRD_BUILD_DIR}/**/libWebView*.a"
        "${LADYBIRD_BUILD_DIR}/**/libladybird_webview*.a"
    )
    if(DEFINED LADYBIRD_LIBRARIES AND LADYBIRD_LIBRARIES)
        target_link_libraries(kakangkung-ladybird-host PRIVATE ${LADYBIRD_LIBRARIES})
    elseif(_LB_WEBVIEWS)
        message(STATUS "Linking discovered LibWebView: ${_LB_WEBVIEWS}")
        target_link_libraries(kakangkung-ladybird-host PRIVATE ${_LB_WEBVIEWS})
    else()
        message(WARNING "HAVE_LADYBIRD=ON but no LibWebView archive found yet under ${LADYBIRD_BUILD_DIR}; set LADYBIRD_LIBRARIES after Ladybird finishes building.")
    endif()
    if(UNIX AND NOT APPLE)
        find_package(X11 REQUIRED)
        target_link_libraries(kakangkung-ladybird-host PRIVATE X11::X11)
    endif()
endif()

if(WIN32)
    target_link_libraries(kakangkung-ladybird-host PRIVATE user32 gdi32)
endif()

install(TARGETS kakangkung-ladybird-host RUNTIME DESTINATION bin)
