cmake_minimum_required(VERSION 3.16)

# 镜像的版本来自 `git describe`，而它必须装得进镜像。
#
# `esp_app_desc_t.version` 是 32 字节，ESP-IDF 会悄悄截断到它 —— 而这段截断是**界面**看不出来的：
# `pomelo-hal` 的 `FirmwareInfo::release` 靠看**字符串的结尾**（一段提交数 + 一个 `g` 开头的哈希）
# 才能把它收窄成 tag，而被从中间剪断的字符串它一个标记都认不出，于是面板画出来的是整串尾巴而不是
# 版本号。`v0.1.1-20261010-16-gf7c258c-dirty` 是 33 个字符，这就是那件事的来由。
#
# 于是这里做两件事：哈希取 git 允许的最短（四位十六进制 —— 这一次构建具体是哪个提交属于构建自己的
# 事，而它的编译时间就写在同一块面板上），以及万一连这样都装不下，就只留 tag。**永远不让出去的是
# `-dirty`**：它是唯一一个「这份代码不是 tag 那份」的标记，也正是界面在收窄掉其余部分之后留下的那个。
if(NOT DEFINED PROJECT_VER)
    execute_process(
        COMMAND git describe --tags --always --dirty --abbrev=4
        WORKING_DIRECTORY "${CMAKE_CURRENT_LIST_DIR}"
        OUTPUT_VARIABLE describe
        OUTPUT_STRIP_TRAILING_WHITESPACE
        ERROR_QUIET
        RESULT_VARIABLE git_status
    )

    if(git_status EQUAL 0 AND describe)
        # 31 而不是 32：那 32 个字节里还有一个结尾的 NUL。
        string(LENGTH "${describe}" describe_length)

        if(describe_length GREATER 31)
            execute_process(
                COMMAND git describe --tags --dirty --abbrev=0
                WORKING_DIRECTORY "${CMAKE_CURRENT_LIST_DIR}"
                OUTPUT_VARIABLE describe
                OUTPUT_STRIP_TRAILING_WHITESPACE
                ERROR_QUIET
            )
        endif()

        set(PROJECT_VER "${describe}")
    endif()
    # git 不在或这次不是在仓库里构建时什么都不设：`project.cmake` 会按它自己那套去问，
    # 问不到就退成 `1`，那也是一种诚实的答案。
endif()

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(firmware)
