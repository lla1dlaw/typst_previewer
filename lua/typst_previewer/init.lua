local M = {}
local job_id = nil

--- Resolve the compiled binary path relative to the plugin root.
local function get_binary_path()
  local root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h:h")
  local is_win = vim.uv.os_uname().sysname:find("Windows") ~= nil
  local bin = is_win and "typst-previewer.exe" or "typst-previewer"
  return root .. "/target/release/" .. bin
end

--- Open the preview window for the current buffer.
function M.open()
  if job_id then
    vim.notify("Typst preview already running", vim.log.levels.WARN)
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
    -- on_lines(_, bufnr, changedtick, firstline, lastline, new_lastline, ...)
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
