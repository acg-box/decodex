#!/usr/bin/env swift
import AppKit
import Foundation

// Public AppKit icon rendering. A process-local UserDefaults argument selects
// the appearance; no system preference is written and no global theme changes.
precondition(CommandLine.arguments.count>=3,"Usage: render_liquid_glass_appearance.swift APP OUTPUT [-AppleIconAppearanceTheme THEME]")
let app=CommandLine.arguments[1],output=CommandLine.arguments[2]
let theme=UserDefaults.standard.string(forKey:"AppleIconAppearanceTheme") ?? "system"
let appearance=NSAppearance(named:theme.hasSuffix("Dark") ? .darkAqua : .aqua)!
let rep=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:1024,pixelsHigh:1024,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
NSGraphicsContext.saveGraphicsState();NSGraphicsContext.current=NSGraphicsContext(bitmapImageRep:rep)
appearance.performAsCurrentDrawingAppearance {
    let icon=NSWorkspace.shared.icon(forFile:app)
    icon.draw(in:NSRect(x:0,y:0,width:1024,height:1024))
}
NSGraphicsContext.restoreGraphicsState()
try rep.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:output))
print(UserDefaults.standard.string(forKey:"AppleIconAppearanceTheme") ?? "system")
