-- How fast the game is drawing, in the top right corner. Its key (F12) goes
-- round three ways of showing it:
--
--   off
--   compact  one line: frames a second, the time of the last frame, the
--            1% low and the slowest frame
--   full     the same numbers larger, and a graph of the last 240 frames
--            against lines at 60 and 30 frames a second
--
-- Each number is green when it would hold 60 frames a second, yellow when
-- it would hold 30, and red below that. The 1% low is the frame rate of the
-- slowest hundredth of the last 1000 frames: it says how smooth the game
-- feels, which the average does not.

local meter = {}

meter.modes = { "off", "compact", "full" }

-- Kept, so a save of the script does not change it.
local kept = game.keep("frame meter", { mode = "off" })

local GOOD, FAIR, POOR = "#7ee08a", "#ffd25a", "#ff6a5a"
local QUIET = "#ffffffa0"

-- The colour for a number of frames a second, and for a frame's time.
local function rateColor(rate: number): string
    return if rate >= 59 then GOOD elseif rate >= 29 then FAIR else POOR
end
local function timeColor(seconds: number): string
    return rateColor(if seconds > 0 then 1 / seconds else 0)
end

local compactBack = game.hud.box({ at = "top right", right = -12, down = 12, width = 640, height = 40, color = "#0c0e1399" })
local compact = game.hud.text("", { at = "top right", right = -24, down = 16, size = 26 })

-- The full meter: a dark panel, four lines of numbers, the graph and what
-- the graph's lines mean.
local back = game.hud.box({ at = "top right", right = -12, down = 12, width = 440, height = 300, color = "#0c0e13d0" })
local rate = game.hud.text("", { at = "top right", right = -32, down = 22, size = 44 })
local last = game.hud.text("", { at = "top right", right = -32, down = 76, size = 26 })
local low = game.hud.text("", { at = "top right", right = -32, down = 108, size = 26 })
local slowest = game.hud.text("", { at = "top right", right = -32, down = 140, size = 26 })
local graph = game.hud.frameGraph({ at = "top right", right = -32, down = 180, width = 400, height = 80, frames = 240 })
local legend = game.hud.text("last 240 frames; lines at 60 and 30 fps", { at = "top right", right = -32, down = 266, size = 24, color = QUIET })
local full = { back, rate, last, low, slowest, graph, legend }

-- What is shown now: "off", "compact" or "full".
function meter.mode(): string
    return kept.mode
end

-- Shows the meter one of the three ways.
function meter.setMode(mode: string)
    kept.mode = mode
end

-- The next way round: off, compact, full, off.
function meter.cycle()
    local now = table.find(meter.modes, kept.mode) or 1
    kept.mode = meter.modes[now % #meter.modes + 1]
end

-- Shows the numbers as they are now. Call it from game.onDraw: the frames
-- can only be read while drawing.
function meter.draw()
    compact.visible = kept.mode == "compact"
    compactBack.visible = compact.visible
    for _, piece in full do
        piece.visible = kept.mode == "full"
    end
    if kept.mode == "off" then
        return
    end
    local fps, lowFps = game.frames.rate(), game.frames.lowRate()
    local lastTime, worst = game.frames.lastTime(), game.frames.slowestTime()
    if kept.mode == "compact" then
        compact.text = string.format(
            "%d fps   %.1f ms   1%% low %d   slowest %.1f ms",
            math.floor(fps + 0.5),
            lastTime * 1000,
            math.floor(lowFps + 0.5),
            worst * 1000
        )
        -- One line has one colour: the worse of the average and the 1% low.
        compact.color = rateColor(math.min(fps, lowFps))
        return
    end
    rate.text = string.format("%d fps", math.floor(fps + 0.5))
    rate.color = rateColor(fps)
    last.text = string.format("%.1f ms a frame", lastTime * 1000)
    last.color = timeColor(lastTime)
    low.text = string.format("1%% low  %d fps", math.floor(lowFps + 0.5))
    low.color = rateColor(lowFps)
    slowest.text = string.format("slowest  %.1f ms", worst * 1000)
    slowest.color = timeColor(worst)
end

return meter
