-- How the planetarium looks: its colours, and cards of dark glass to put
-- words on. A card is a few HUD boxes (a soft shadow, the glass, a light
-- edge along its top, and a gold strip if it has one) and is made once and
-- then shown, hidden, moved or faded.

local theme = {}

theme.glass = "#0a1022c8" -- the cards
theme.shadow = "#00000059"
theme.edge = "#ffffff1f" -- the light line along a card's top
theme.gold = "#ffd27a"
theme.ice = "#8fd3ff"
theme.text = "#eef3ff"
theme.muted = "#9fb0d4"
theme.faint = "#9fb0d440"

-- `color` with its see-through part times `share` (0 gone, 1 as it is). A
-- colour is "#rrggbb" or "#rrggbbaa".
function theme.fade(color: string, share: number): string
    local base = string.sub(color, 1, 7)
    local alpha = if #color >= 9 then tonumber(string.sub(color, 8, 9), 16) or 255 else 255
    return string.format("%s%02x", base, math.floor(alpha * math.clamp(share, 0, 1) + 0.5))
end

local Card = {}
Card.__index = Card

export type Card = typeof(setmetatable(
    {} :: {
        at: string,
        right: number,
        down: number,
        width: number,
        height: number,
        boxes: { { piece: HudBox, color: string } },
        texts: { { piece: HudText, color: string } },
    },
    Card
))

-- Where a piece `width` by `height` goes so that its top left corner is
-- `across` and `down` from the card's top left corner, given that every
-- piece is pinned to the card's own place on the screen.
function Card.offset(self: Card, across: number, down: number, width: number, height: number): (number, number)
    local at = self.at
    local right = self.right + across
    if string.find(at, "right") then
        right = self.right + across - self.width + width
    elseif at == "top" or at == "bottom" or at == "center" then
        right = self.right + across - self.width / 2 + width / 2
    end
    local lower = self.down + down
    if string.find(at, "bottom") then
        lower = self.down + down - self.height + height
    elseif at == "left" or at == "right" or at == "center" then
        lower = self.down + down - self.height / 2 + height / 2
    end
    return right, lower
end

-- A box inside the card.
function Card.box(self: Card, across: number, down: number, width: number, height: number, color: string): HudBox
    local right, lower = self:offset(across, down, width, height)
    local piece = game.hud.box({ at = self.at :: any, right = right, down = lower, width = width, height = height, color = color })
    table.insert(self.boxes, { piece = piece, color = color })
    return piece
end

-- Words inside the card, `down` from its top. The HUD sizes text itself, so
-- which edge of the words `across` says depends on where the card is
-- pinned: their left edge on a card at the left of the screen, their right
-- edge on one at the right, their middle on one in the middle.
function Card.text(self: Card, text: string, across: number, down: number, size: number, color: string): HudText
    local piece = game.hud.text(text, { at = self.at :: any, size = size, color = color })
    table.insert(self.texts, { piece = piece, color = color })
    self:placeText(piece, across, down, size)
    return piece
end

-- Moves words already in the card, as Card.text places them. Its height
-- is taken as 1.25 times its size, which is near enough for the HUD's font.
function Card.placeText(self: Card, piece: HudText, across: number, down: number, size: number)
    local at = self.at
    local height = size * 1.25
    local pinnedRight = string.find(at, "right") ~= nil
    local pinnedMiddle = at == "top" or at == "bottom" or at == "center"
    local x
    if pinnedRight then
        x = self.right - self.width + across
    elseif pinnedMiddle then
        x = self.right + across - self.width / 2
    else
        x = self.right + across
    end
    local _, lower = self:offset(0, down, 0, height)
    piece.right = x
    piece.down = lower
end

-- Shows or hides the whole card.
function Card.setVisible(self: Card, visible: boolean)
    for _, each in self.boxes do
        each.piece.visible = visible
    end
    for _, each in self.texts do
        each.piece.visible = visible
    end
end

-- Fades the whole card: 1 as made, 0 gone.
function Card.setFade(self: Card, share: number)
    for _, each in self.boxes do
        each.piece.color = theme.fade(each.color, share)
    end
    for _, each in self.texts do
        each.piece.color = theme.fade(each.color, share)
    end
end

-- Makes a card of glass, hidden until shown. `at` is where it is pinned on
-- the screen, as game.hud takes it; `right` and `down` move it from there.
-- With `accent`, a strip of that colour runs down its left side.
function theme.card(settings: { at: string, right: number?, down: number?, width: number, height: number, accent: string?, shadow: boolean? }): Card
    local self = setmetatable({
        at = settings.at,
        right = settings.right or 0,
        down = settings.down or 0,
        width = settings.width,
        height = settings.height,
        boxes = {},
        texts = {},
    }, Card)
    if settings.shadow ~= false then
        self:box(5, 7, settings.width, settings.height, theme.shadow)
    end
    self:box(0, 0, settings.width, settings.height, theme.glass)
    self:box(0, 0, settings.width, 1, theme.edge)
    if settings.accent then
        self:box(0, 0, 3, settings.height, settings.accent)
    end
    return self
end

return theme
