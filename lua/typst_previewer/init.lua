local M = {}
local job_id = nil

--- Default options for the plugin
M.options = {
  binary_path = nil,    -- Custom binary path or nil for auto-discovery
  auto_download = true, -- Automatically download pre-built release binary from GitHub if missing
  auto_resize = true,   -- Automatically adjust window size to document dimensions and font scaling
  max_width = 1400,     -- Maximum auto-resized window width in points
  max_height = 1100,    -- Maximum auto-resized window height in points
  scale_factor = nil,   -- Custom display scale override or nil (auto-detect from OS display)
  system_fonts = true,  -- Discover and load system fonts across Linux, macOS, and Windows
  keymaps = {
    toggle = "<leader>tp",
    open = nil,
    close = nil,
  },
}

--- Resolves the compiled binary path across multiple candidate locations.
local function get_binary_path()
  if M.options.binary_path and vim.fn.executable(M.options.binary_path) == 1 then
    return M.options.binary_path
  end

  local root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h:h")
  local is_win = vim.uv.os_uname().sysname:find("Windows") ~= nil
  local bin = is_win and "typst-previewer.exe" or "typst-previewer"

  -- 1. Check plugin root target/release/
  local release_bin = root .. "/target/release/" .. bin
  if vim.fn.executable(release_bin) == 1 then
    return release_bin
  end

  -- 2. Check plugin root target/debug/
  local debug_bin = root .. "/target/debug/" .. bin
  if vim.fn.executable(debug_bin) == 1 then
    return debug_bin
  end

  -- 3. Check system PATH
  local path_bin = vim.fn.exepath(bin)
  if path_bin ~= "" and vim.fn.executable(path_bin) == 1 then
    return path_bin
  end

  return release_bin
end

--- Detects current platform and returns the GitHub release asset info.
local function get_platform_asset()
  local uname = vim.uv.os_uname()
  local sys = uname.sysname:lower()
  local machine = uname.machine:lower()

  local os_name, ext, arch
  if sys:find("linux") then
    os_name = "linux"
    ext = "tar.gz"
  elseif sys:find("darwin") then
    os_name = "macos"
    ext = "tar.gz"
  elseif sys:find("windows") or sys:find("mingw") then
    os_name = "windows"
    ext = "zip"
  else
    return nil, "Unsupported operating system: " .. sys
  end

  if machine == "x86_64" or machine == "amd64" then
    arch = "x86_64"
  elseif machine == "arm64" or machine == "aarch64" then
    arch = "aarch64"
  else
    return nil, "Unsupported architecture: " .. machine
  end

  local asset_name = string.format("typst-previewer-%s-%s.%s", os_name, arch, ext)
  return {
    os = os_name,
    arch = arch,
    ext = ext,
    asset = asset_name,
    url = "https://github.com/lla1dlaw/typst_previewer/releases/latest/download/" .. asset_name,
  }
end

--- Downloads and installs the pre-built release binary from GitHub.
--- Can be called with `M.install()` or `M.install({ sync = true })` or `M.install(callback, { sync = true })`.
function M.install(callback, opts)
  if type(callback) == "table" and opts == nil then
    opts = callback
    callback = nil
  end
  opts = opts or {}
  callback = callback or function() end

  local info, err = get_platform_asset()
  if not info then
    vim.notify("Typst previewer: " .. (err or "Unknown platform error"), vim.log.levels.ERROR)
    callback(false)
    return
  end

  local root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h:h")
  local target_dir = root .. "/target/release"
  vim.fn.mkdir(target_dir, "p")

  local is_win = info.os == "windows"
  local temp_file = target_dir .. "/download." .. info.ext
  local bin_name = is_win and "typst-previewer.exe" or "typst-previewer"
  local bin_path = target_dir .. "/" .. bin_name

  vim.notify("Typst previewer: Downloading pre-built binary for " .. info.os .. " (" .. info.arch .. ")...", vim.log.levels.INFO)

  local cmd
  if is_win then
    cmd = string.format(
      "curl -sL --fail \"%s\" -o \"%s\" && tar -xf \"%s\" -C \"%s\" && del /f /q \"%s\"",
      info.url, temp_file, temp_file, target_dir, temp_file
    )
  else
    cmd = string.format(
      "curl -sL --fail \"%s\" -o \"%s\" && tar -xzf \"%s\" -C \"%s\" && chmod +x \"%s\" && rm -f \"%s\"",
      info.url, temp_file, temp_file, target_dir, bin_path, temp_file
    )
  end

  local is_headless = #vim.api.nvim_list_uis() == 0
  local jid = vim.fn.jobstart(cmd, {
    shell = true,
    on_exit = function(_, exit_code)
      if exit_code == 0 and vim.fn.executable(bin_path) == 1 then
        vim.notify("Typst previewer: Successfully installed binary!", vim.log.levels.INFO)
        callback(true)
      else
        vim.notify(
          "Typst previewer: Download failed. You can build manually with `cargo build --release` in " .. root,
          vim.log.levels.WARN
        )
        callback(false)
      end
    end,
  })

  if (opts.sync or is_headless) and jid > 0 then
    vim.fn.jobwait({ jid })
  end
end

--- Setup function for lazy.nvim or custom user configuration.
function M.setup(opts)
  opts = opts or {}
  M.options = vim.tbl_deep_extend("force", M.options, opts)

  if M.options.keymaps.toggle then
    vim.keymap.set("n", M.options.keymaps.toggle, function() M.toggle() end, { desc = "Toggle Typst Preview" })
  end
  if M.options.keymaps.open then
    vim.keymap.set("n", M.options.keymaps.open, function() M.open() end, { desc = "Open Typst Preview" })
  end
  if M.options.keymaps.close then
    vim.keymap.set("n", M.options.keymaps.close, function() M.close() end, { desc = "Close Typst Preview" })
  end
end

--- Handles click-to-source jump notification from the previewer.
function M.jump_to(filepath, line, col)
  vim.schedule(function()
    line = math.max(1, tonumber(line) or 1)
    col = math.max(0, tonumber(col) or 0)

    -- If a specific file path is targeted and different from current buffer, switch window/buffer
    if filepath and filepath ~= "" then
      local current = vim.api.nvim_buf_get_name(0)
      if current ~= filepath then
        -- Check if any window already shows this file
        for _, win in ipairs(vim.api.nvim_list_wins()) do
          local b = vim.api.nvim_win_get_buf(win)
          if vim.api.nvim_buf_get_name(b) == filepath then
            vim.api.nvim_set_current_win(win)
            pcall(vim.api.nvim_win_set_cursor, win, { line, col })
            vim.cmd("normal! zz")
            return
          end
        end

        -- Check existing loaded buffers
        for _, b in ipairs(vim.api.nvim_list_bufs()) do
          if vim.api.nvim_buf_get_name(b) == filepath then
            vim.api.nvim_set_current_buf(b)
            pcall(vim.api.nvim_win_set_cursor, 0, { line, col })
            vim.cmd("normal! zz")
            return
          end
        end
      end
    end

    local max_lines = vim.api.nvim_buf_line_count(0)
    line = math.min(line, max_lines)
    pcall(vim.api.nvim_win_set_cursor, 0, { line, col })
    vim.cmd("normal! zz")
  end)
end

--- Open the preview window for the current buffer.
function M.open()
  if job_id then
    vim.notify("Typst preview is already running", vim.log.levels.WARN)
    return
  end

  local bin = get_binary_path()
  if vim.fn.executable(bin) ~= 1 then
    if M.options.auto_download then
      M.install(function(success)
        if success then
          M.open()
        end
      end)
      return
    else
      vim.notify(
        "Typst previewer binary not found at: " .. bin .. "\nRun `:TypstPreviewInstall` or `cargo build --release`.",
        vim.log.levels.ERROR
      )
      return
    end
  end

  -- Spawn binary with msgpack-RPC over stdio
  job_id = vim.fn.jobstart({ bin }, {
    rpc = true,
    on_exit = function(_, code)
      job_id = nil
      if code ~= 0 then
        vim.notify("Typst previewer exited with code " .. code, vim.log.levels.WARN)
      end
    end,
  })

  if job_id <= 0 then
    vim.notify("Failed to start typst previewer", vim.log.levels.ERROR)
    job_id = nil
    return
  end

  -- Send the full initial buffer content, file path, and active configuration
  local bufnr = vim.api.nvim_get_current_buf()
  local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
  local filepath = vim.api.nvim_buf_get_name(bufnr)

  vim.rpcnotify(job_id, "init_buffer", {
    content = table.concat(lines, "\n"),
    filepath = filepath,
    config = {
      auto_resize = M.options.auto_resize,
      max_width = M.options.max_width,
      max_height = M.options.max_height,
      scale_factor = M.options.scale_factor,
      system_fonts = M.options.system_fonts,
    },
  })

  -- Attach to buffer for real-time delta updates
  vim.api.nvim_buf_attach(bufnr, false, {
    on_lines = function(_, buf, _, firstline, lastline, new_lastline)
      if not job_id then return true end -- returning true detaches
      local new_lines = vim.api.nvim_buf_get_lines(buf, firstline, new_lastline, false)
      vim.rpcnotify(job_id, "buffer_lines", {
        firstline = firstline,
        lastline = lastline,
        lines = new_lines,
      })
    end,
    on_detach = function()
      M.close()
    end,
  })
end

--- Close the preview window.
function M.close()
  if job_id then
    vim.fn.jobstop(job_id)
    job_id = nil
  end
end

--- Toggle the preview window.
function M.toggle()
  if job_id then
    M.close()
  else
    M.open()
  end
end

-- User commands
vim.api.nvim_create_user_command("TypstPreview", function() M.open() end, { desc = "Open Typst Preview" })
vim.api.nvim_create_user_command("TypstPreviewClose", function() M.close() end, { desc = "Close Typst Preview" })
vim.api.nvim_create_user_command("TypstPreviewToggle", function() M.toggle() end, { desc = "Toggle Typst Preview" })
vim.api.nvim_create_user_command("TypstPreviewInstall", function() M.install() end, { desc = "Install pre-built Typst Previewer binary" })

-- Clean up process on Neovim exit
vim.api.nvim_create_autocmd("VimLeavePre", {
  callback = function()
    M.close()
  end,
})

return M
