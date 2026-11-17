local M = {}
local job_id = nil

M.options = {
  binary_path = nil, -- Optional custom path override to the executable
  keymaps = {
    toggle = nil, -- e.g. "<leader>tp"
    open = nil,   -- e.g. "<leader>to"
    close = nil,  -- e.g. "<leader>tc"
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
    vim.notify(
      "Typst previewer binary not found at: " .. bin .. "\nRun `cargo build --release` in the plugin directory.",
      vim.log.levels.ERROR
    )
    return
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

  -- Send the full initial buffer content and file path
  local bufnr = vim.api.nvim_get_current_buf()
  local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
  local filepath = vim.api.nvim_buf_get_name(bufnr)

  vim.rpcnotify(job_id, "init_buffer", {
    content = table.concat(lines, "\n"),
    filepath = filepath,
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
vim.api.nvim_create_user_command("TypstPreview", function() M.open() end, {})
vim.api.nvim_create_user_command("TypstPreviewClose", function() M.close() end, {})
vim.api.nvim_create_user_command("TypstPreviewToggle", function() M.toggle() end, {})

return M
