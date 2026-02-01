#!/usr/bin/env python3
"""Generate color palette preview for the improved terrain colors"""

from PIL import Image, ImageDraw, ImageFont

def smoothstep(t):
    """Smooth interpolation function"""
    return t * t * (3.0 - 2.0 * t)

def lerp_color(c1, c2, t):
    """Interpolate between two colors with smoothstep"""
    t_smooth = smoothstep(t)
    return tuple(int(c1[i] + (c2[i] - c1[i]) * t_smooth) for i in range(3))

def interpolate_gradient(stops, ratio):
    """Interpolate color from a list of (position, color) stops"""
    ratio = max(0.0, min(1.0, ratio))
    
    for i in range(len(stops) - 1):
        pos1, color1 = stops[i]
        pos2, color2 = stops[i + 1]
        
        if pos1 <= ratio <= pos2:
            t = (ratio - pos1) / (pos2 - pos1)
            return lerp_color(color1, color2, t)
    
    return stops[-1][1]

def height_to_color(height):
    """Convert height (0-255) to RGB color"""
    SEA_LEVEL = 20
    
    ratio = height / 255.0
    sea_ratio = SEA_LEVEL / 255.0
    
    if height < SEA_LEVEL:
        # Ocean gradient
        ocean_stops = [
            (0.0, (8, 24, 58)),       # Deep ocean
            (0.3, (16, 48, 120)),     # Mid-deep
            (0.7, (32, 80, 170)),     # Shallow
            (1.0, (60, 120, 190)),    # Coastal
        ]
        ocean_ratio = ratio / sea_ratio
        return interpolate_gradient(ocean_stops, ocean_ratio)
    else:
        # Land gradient with smooth transitions
        land_stops = [
            (0.0, (210, 180, 140)),   # Beach/coast
            (0.05, (34, 120, 50)),    # Dark green (forest)
            (0.15, (50, 150, 50)),    # Mid green
            (0.25, (100, 170, 60)),   # Light green
            (0.35, (160, 180, 70)),   # Yellow-green (grassland)
            (0.45, (200, 170, 80)),   # Yellow/khaki (dry grass)
            (0.55, (180, 130, 70)),   # Orange-brown (low mountain)
            (0.70, (130, 100, 70)),   # Dark brown (mountain)
            (0.85, (150, 145, 140)),  # Gray (rock)
            (1.0, (255, 255, 255)),   # White (snow)
        ]
        land_ratio = (ratio - sea_ratio) / (1.0 - sea_ratio)
        return interpolate_gradient(land_stops, land_ratio)

def main():
    # Create image
    width = 800
    height = 400
    img = Image.new('RGB', (width, height), (255, 255, 255))
    draw = ImageDraw.Draw(img)
    
    # Draw color gradient bar (full width)
    bar_height = 100
    bar_y = 50
    
    for x in range(width):
        h = int(x * 255 / width)
        color = height_to_color(h)
        draw.line([(x, bar_y), (x, bar_y + bar_height)], fill=color)
    
    # Draw border
    draw.rectangle([0, bar_y, width-1, bar_y + bar_height], outline=(0, 0, 0))
    
    # Add labels
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 14)
        font_small = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 12)
    except:
        font = ImageFont.load_default()
        font_small = font
    
    # Title
    draw.text((width//2 - 150, 10), "Sekai Terrain Color Palette (Improved)", fill=(0, 0, 0), font=font)
    
    # Labels below the bar
    labels = [
        (0, "Deep Sea"),
        (int(20 * width / 255), "Shore"),
        (int(50 * width / 255), "Forest"),
        (int(100 * width / 255), "Grassland"),
        (int(150 * width / 255), "Hills"),
        (int(200 * width / 255), "Mountain"),
        (int(240 * width / 255), "Snow"),
    ]
    
    for x, label in labels:
        draw.line([(x, bar_y + bar_height), (x, bar_y + bar_height + 10)], fill=(0, 0, 0))
        draw.text((x - 20, bar_y + bar_height + 15), label, fill=(0, 0, 0), font=font_small)
    
    # Draw comparison: Old vs New
    compare_y = 220
    section_width = width // 2 - 20
    
    # Old colors (approximate)
    old_land_stops = [
        (0.0, (34, 139, 34)),     # Green
        (0.33, (134, 89, 14)),    # Direct jump to brown
        (0.66, (174, 119, 24)),   # Light brown
        (1.0, (255, 255, 255)),   # White
    ]
    
    draw.text((10, compare_y - 25), "Before (abrupt green→brown):", fill=(0, 0, 0), font=font)
    for x in range(section_width):
        ratio = x / section_width
        if ratio < 0.1:
            color = (60, 120, 190)  # Ocean
        else:
            land_r = (ratio - 0.1) / 0.9
            color = interpolate_gradient(old_land_stops, land_r)
        draw.line([(x + 10, compare_y), (x + 10, compare_y + 40)], fill=color)
    
    # New colors
    draw.text((width//2 + 10, compare_y - 25), "After (smooth gradient):", fill=(0, 0, 0), font=font)
    for x in range(section_width):
        h = int(x * 255 / section_width)
        color = height_to_color(h)
        draw.line([(x + width//2 + 10, compare_y), (x + width//2 + 10, compare_y + 40)], fill=color)
    
    # Add template count
    draw.text((10, height - 60), "Templates: 22 total (8 original + 10 Azgaar-style + 4 primitive-based)", 
              fill=(0, 0, 0), font=font)
    
    templates = [
        "Volcano, High Island, Low Island, Continents, Archipelago (Azgaar),",
        "Atoll (Azgaar), Mediterranean, Peninsula (Azgaar), Pangea, Isthmus"
    ]
    draw.text((10, height - 40), f"New Azgaar-style: {templates[0]}", fill=(100, 100, 100), font=font_small)
    draw.text((10, height - 25), f"                  {templates[1]}", fill=(100, 100, 100), font=font_small)
    
    # Save
    img.save('/root/sekai/color_preview.png')
    print("Color preview saved to /root/sekai/color_preview.png")

if __name__ == "__main__":
    main()
