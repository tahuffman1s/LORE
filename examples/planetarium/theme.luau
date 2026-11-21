-- How the planetarium looks: one palette, one scale of text sizes, and
-- panels of dark glass to put words on. A panel is a few HUD boxes and its
-- words, made once and then shown, hidden, moved or faded.
--
-- Everything is laid out in pixels on a screen 1080 high, against the edge
-- of the screen it is pinned to, so it stays put on any screen.

local theme = {}

-- Colours.
theme.text = "#f2f5fb"
theme.muted = "#8f9cbc"
theme.faint = "#8f9cbc55"
theme.accent = "#e9c27a" -- gold, for what matters now
theme.glass = "#070b18b8"
theme.outline = "#ffffff16"
theme.shadow = "#00000066"

-- Text sizes, smallest first: labels in capitals, small print, body,
-- titles, and the clock.
theme.micro = 13
theme.small = 16
theme.body = 19
theme.title = 26
theme.display = 44

-- How round panels are, and how far words sit in from their edges.
theme.corner = 10
theme.inset = 20

-- `color` with its see-through part times `share` (0 gone, 1 as it is). A
-- colour is "#rrggbb" or "#rrggbbaa".
function theme.fade(color: string, share: number): string
    local base = string.sub(color, 1, 7)
    local alpha = if #color >= 9 then tonumber(string.sub(color, 8, 9), 16) or 255 else 255
    return string.format("%s%02x", base, math.floor(alpha * math.clamp(share, 0, 1) + 0.5))
end

local Panel = {}
Panel.__index = Panel

export type Panel = typeof(setmetatable(
    {} :: {
        at: string,
        right: number,
        down: number,
        width: number,
        height: number,
        boxes: { { piece: HudBox, color: string } },
        texts: { { piece: HudText, color: string } },
        -- Its shadow, outline and glass, which follow its size; none if bare.
        frame: { HudBox },
        corner: number,
    },
    Panel
))

-- How far across and down the screen a place is: 0, a half or 1.
local function fractions(at: string): (number, number)
    local across = if string.find(at, "left") then 0 elseif string.find(at, "right") then 1 else 0.5
    local down = if string.find(at, "top") then 0 elseif string.find(at, "bottom") then 1 else 0.5
    return across, down
end

-- The offset that puts a piece `width` by `height` with its top left corner
-- `across` and `down` from the panel's top left corner. Every piece is
-- pinned where the panel is, and a piece pinned a share `f` of the way
-- across the screen sits `f` of its own width further left.
function Panel.offset(self: Panel, across: number, down: number, width: number, height: number): (number, number)
    local fx, fy = fractions(self.at)
    return self.right + across - fx * (self.width - width), self.down + down - fy * (self.height - height)
end

-- A box inside the panel. With `corner`, its corners are rounded.
function Panel.box(self: Panel, across: number, down: number, width: number, height: number, color: string, corner: number?): HudBox
    local right, lower = self:offset(across, down, width, height)
    local piece = game.hud.box({ at = self.at :: any, right = right, down = lower, width = width, height = height, color = color, corner = corner })
    table.insert(self.boxes, { piece = piece, color = color })
    return piece
end

-- Moves and resizes a box made with Panel.box.
function Panel.placeBox(self: Panel, piece: HudBox, across: number, down: number, width: number, height: number)
    piece.width, piece.height = width, height
    piece.right, piece.down = self:offset(across, down, width, height)
end

-- Words inside the panel, their top `down` from the panel's top. `align`
-- says which edge of the words `across` is: "left" (the usual), "right" or
-- "center". The words are measured, so any of the three is exact.
function Panel.text(self: Panel, text: string, across: number, down: number, size: number, color: string, align: string?): HudText
    local piece = game.hud.text(text, { at = self.at :: any, size = size, color = color })
    table.insert(self.texts, { piece = piece, color = color })
    self:placeText(piece, across, down, align)
    return piece
end

-- Moves words made with Panel.text, as it places them. Call it again after
-- changing the words of any but left-aligned text on a left-pinned panel.
function Panel.placeText(self: Panel, piece: HudText, across: number, down: number, align: string?)
    local width, height = piece.width, piece.height
    local left = across
    if align == "right" then
        left = across - width
    elseif align == "center" then
        left = across - width / 2
    end
    piece.right, piece.down = self:offset(left, down, width, height)
end

-- Changes the panel's size, keeping it pinned where it is: its shadow,
-- outline and glass follow. Its other pieces are placed by the caller, as
-- where they go depends on the size.
function Panel.resize(self: Panel, width: number, height: number)
    self.width, self.height = width, height
    local frame = self.frame
    if #frame == 3 then
        self:placeBox(frame[1], 0, 6, width, height)
        self:placeBox(frame[2], -1, -1, width + 2, height + 2)
        self:placeBox(frame[3], 0, 0, width, height)
    end
end

-- Moves the whole panel, from where it is pinned.
function Panel.moveTo(self: Panel, right: number, down: number)
    self.right, self.down = right, down
    self:resize(self.width, self.height)
end

-- Puts `text` in `piece`, shortened with an ellipsis if it would be wider
-- than `most`. Gives back whether it had to be shortened.
function theme.fit(piece: HudText, text: string, most: number): boolean
    piece.text = text
    if piece.width <= most then
        return false
    end
    local length = utf8.len(text) or #text
    while length > 1 do
        length -= 1
        local cut = string.sub(text, 1, (utf8.offset(text, length + 1) or (#text + 1)) - 1)
        piece.text = string.gsub(cut, "[%s,·]+$", "") .. "…"
        if piece.width <= most then
            break
        end
    end
    return true
end

function Panel.setVisible(self: Panel, visible: boolean)
    for _, each in self.boxes do
        each.piece.visible = visible
    end
    for _, each in self.texts do
        each.piece.visible = visible
    end
end

-- Fades the whole panel: 1 as made, 0 gone.
function Panel.setFade(self: Panel, share: number)
    for _, each in self.boxes do
        each.piece.color = theme.fade(each.color, share)
    end
    for _, each in self.texts do
        each.piece.color = theme.fade(each.color, share)
    end
end

-- A panel, hidden until shown. `at` is where it is pinned on the screen, as
-- game.hud takes it; `right` and `down` move it from there. Unless told
-- `bare`, it has a soft shadow, dark glass and a faint outline.
function theme.panel(settings: { at: string, right: number?, down: number?, width: number, height: number, bare: boolean?, corner: number? }): Panel
    local self = setmetatable({
        at = settings.at,
        right = settings.right or 0,
        down = settings.down or 0,
        width = settings.width,
        height = settings.height,
        boxes = {},
        texts = {},
        frame = {},
        corner = settings.corner or theme.corner,
    }, Panel)
    if not settings.bare then
        local corner = self.corner
        local w, h = settings.width, settings.height
        self.frame = {
            self:box(0, 6, w, h, theme.shadow, corner),
            -- A one-pixel outline: a faint box, and the glass one pixel inside it.
            self:box(-1, -1, w + 2, h + 2, theme.outline, corner + 1),
            self:box(0, 0, w, h, theme.glass, corner),
        }
    end
    self:setVisible(false)
    return self
end

return theme
