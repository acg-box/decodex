#!/usr/bin/env swift
import AppKit
import Foundation

// Regression checks at real menu-bar raster sizes. Cutout mode checks one
// foreground cloud plus background and two independent transparent holes.
// Include low-alpha antialias pixels: a 50% threshold alone can hide bridges.
let cutout=CommandLine.arguments.contains("--cutout")
let source=NSImage(contentsOfFile:CommandLine.arguments[1])!
for size in [22,44] { for phaseX:CGFloat in [0,0.5] { for phaseY:CGFloat in [0,0.5] {
 let rep=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:size,pixelsHigh:size,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
 NSGraphicsContext.saveGraphicsState();NSGraphicsContext.current=NSGraphicsContext(bitmapImageRep:rep)
 source.draw(in:NSRect(x:phaseX,y:phaseY,width:CGFloat(size),height:CGFloat(size)));NSGraphicsContext.restoreGraphicsState()
 for threshold:CGFloat in [0.25,0.5] { for holes in (cutout ? [false,true] : [false]) {
  var seen=Set<Int>(),counts=[Int]()
  for y in 0..<size { for x in 0..<size {
   let start=y*size+x
   if seen.contains(start) || (holes ? rep.colorAt(x:x,y:y)!.alphaComponent>=threshold : rep.colorAt(x:x,y:y)!.alphaComponent<threshold) {continue}
   var queue=[start],head=0;seen.insert(start)
   while head<queue.count {let i=queue[head];head+=1
    for dy in -1...1 { for dx in -1...1 {let a=i%size+dx,b=i/size+dy
     if a<0 || a>=size || b<0 || b>=size {continue};let next=b*size+a
     if !seen.contains(next) && (holes ? rep.colorAt(x:a,y:b)!.alphaComponent<threshold : rep.colorAt(x:a,y:b)!.alphaComponent>=threshold) {seen.insert(next);queue.append(next)}
    }}
   }
   counts.append(queue.count)
  }}
  if counts.count != (holes ? 3 : (cutout ? 1 : 3)) {
   FileHandle.standardError.write(Data("Menu icon joins or fragments at \(size)px, offset (\(phaseX), \(phaseY)), alpha \(threshold): \(counts)\n".utf8))
   exit(1)
  }
 }
}

}}}

print("Menu template: \(cutout ? "cutout" : "open-frame") raster checks passed")
