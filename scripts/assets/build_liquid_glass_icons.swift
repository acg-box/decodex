#!/usr/bin/env swift
// Native Icon Composer artwork built from exact geometric primitives.
// Circles, rounded rectangles, tangent caps, and vector boolean operations
// replace photographic contour tracing. RGB paint contains no lighting.
import AppKit
import Foundation
func circle(_ x:CGFloat,_ y:CGFloat,_ r:CGFloat)->CGPath { CGPath(ellipseIn:CGRect(x:x-r,y:y-r,width:r*2,height:r*2),transform:nil) }
let frame=CGPath(rect:CGRect(x:-2048,y:-2048,width:5120,height:5120),transform:nil)
func expand(_ shape:CGPath,_ amount:CGFloat)->CGPath { shape.union(shape.copy(strokingWithWidth:amount*2,lineCap:.round,lineJoin:.round,miterLimit:10)) }
func inset(_ shape:CGPath,_ amount:CGFloat)->CGPath { frame.subtracting(expand(frame.subtracting(shape),amount)) }
func closeCorners(_ shape:CGPath,_ radius:CGFloat)->CGPath { inset(expand(shape,radius),radius).normalized() }
// Offset peak and unequal shoulders make the rounded variant read as a cloud,
// not a symmetric rocket. The open and flat variants keep their own geometry.
let round=circle(463,404,159)
    .union(circle(300,539,148))
    .union(circle(675,480,180))
    .union(circle(408,590,150))
    .union(circle(675,610,145))
    .union(circle(520,615,120))
let capsule=CGPath(roundedRect:CGRect(x:140,y:362,width:744,height:374),cornerWidth:172,cornerHeight:172,transform:nil)
let flat=closeCorners(capsule.union(circle(512,385,174)),16)
let rounded=closeCorners(round,24)
let ring=flat.subtracting(inset(flat,72))
let endX:CGFloat=712+136*cos(.pi/6),endY:CGFloat=564+136*sin(.pi/6)
let cut=CGMutablePath()
cut.move(to:CGPoint(x:710,y:endY+(710-endX)*tan(.pi/6)))
cut.addLine(to:CGPoint(x:1300,y:endY+(1300-endX)*tan(.pi/6)))
cut.addLine(to:CGPoint(x:1300,y:1300));cut.addLine(to:CGPoint(x:710,y:1300));cut.closeSubpath()
let opened=ring.subtracting(cut).union(circle(710,700,36)).union(circle(endX,endY,36)).normalized()
let pts:[CGPoint]=[CGPoint(x:465,y:358),CGPoint(x:326,y:551),CGPoint(x:389,y:551),CGPoint(x:359,y:674),CGPoint(x:510,y:490),CGPoint(x:447,y:490)]
let travel:[CGFloat]=[80,27,10,80,27,10]
let bolt=CGMutablePath()
func approach(_ a:CGPoint,_ b:CGPoint,_ distance:CGFloat)->CGPoint { let dx=b.x-a.x,dy=b.y-a.y,l=hypot(dx,dy),d=min(distance,l-12);return CGPoint(x:a.x+dx/l*d,y:a.y+dy/l*d) }
for i in pts.indices {
 let a=approach(pts[i],pts[(i+5)%6],travel[i]),b=approach(pts[i],pts[(i+1)%6],travel[i])
 if i==0 {bolt.move(to:a)}else{bolt.addLine(to:a)}
 bolt.addQuadCurve(to:b,control:pts[i])
}
bolt.closeSubpath()
let cursor=CGPath(roundedRect:CGRect(x:530,y:bolt.boundingBoxOfPath.maxY-64,width:165,height:64),cornerWidth:32,cornerHeight:32,transform:nil)
func svg(_ path:CGPath)->String {
 var result=[String]()
 func p(_ p:CGPoint)->String {String(format:"%.4f %.4f",p.x,p.y)}
 path.applyWithBlock{ptr in let e=ptr.pointee;switch e.type{case .moveToPoint:result.append("M "+p(e.points[0]));case .addLineToPoint:result.append("L "+p(e.points[0]));case .addQuadCurveToPoint:result.append("Q "+p(e.points[0])+" "+p(e.points[1]));case .addCurveToPoint:result.append("C "+p(e.points[0])+" "+p(e.points[1])+" "+p(e.points[2]));case .closeSubpath:result.append("Z");@unknown default:break}}
 return "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1024\" height=\"1024\" viewBox=\"0 0 1024 1024\"><path fill=\"white\" fill-rule=\"evenodd\" d=\""+result.joined(separator:" ")+"\"/></svg>\n"
}
func color(_ value:String)->[String:String] { ["automatic-gradient":value] }
func specialization(_ appearance:String?,_ value:Any)->[String:Any] {
    var result:[String:Any]=["value":value]
    if let appearance { result["appearance"]=appearance }
    return result
}

let root=URL(fileURLWithPath:FileManager.default.currentDirectoryPath)
let output=root.appendingPathComponent("assets/app-icon/liquid-glass")
let names=["01-mercury-cloud","02-flat-cloud","03-open-cloud"]
for (index,name) in names.enumerated() {
    let directory=output.appendingPathComponent(name)
    let assets=directory.appendingPathComponent("AppIcon.icon/Assets")
    try FileManager.default.createDirectory(at:assets,withIntermediateDirectories:true)
    // Dock optical fit: equal baselines and about 54 points of bottom space.
    let markBounds=bolt.boundingBoxOfPath.union(cursor.boundingBoxOfPath)
    var markTransform=CGAffineTransform(translationX:markBounds.midX,y:markBounds.midY-12)
        .scaledBy(x:0.86,y:0.86).translatedBy(x:-markBounds.midX,y:-markBounds.midY)
    let insetBolt=bolt.copy(using:&markTransform)!
    let insetCursor=cursor.copy(using:&markTransform)!
    let shapes:[CGPath]=index==2 ? [opened,insetBolt,insetCursor] : [(index==0 ? rounded : flat).subtracting(bolt).subtracting(cursor)]
    let iconBounds=(index==0 ? rounded : flat).boundingBoxOfPath
    let dockOffsetX=(512-iconBounds.midX)*0.95
    let dockOffsetY=(512-iconBounds.midY)*0.95
    var groups=[[String:Any]]()
    for (i,shape) in shapes.enumerated() {
        let layerName=index==2 ? ["Cloud frame","Lightning","Cursor"][i] : "Cloud with inset mark"
        let filename="shape-\(i).svg"
        try svg(shape).write(to:assets.appendingPathComponent(filename),atomically:true,encoding:.utf8)
        let layer:[String:Any]=[
            "name":layerName,"image-name":filename,
            "position":["scale":0.95,"translation-in-points":[dockOffsetX,dockOffsetY]],
            "fill-specializations":[
                specialization(nil,color("extended-srgb:0.76,0.84,0.92,1.0")),
                specialization("dark",color("extended-srgb:0.48,0.57,0.67,0.88")),
                specialization("tinted",color("extended-srgb:0.66,0.66,0.66,0.68"))
            ]
        ]
        let depth:Double=index==0 ? 0.22 : (index==2 ? 0.14 : 0.18)
        groups.append([
            "name":layerName,"layers":[layer],"specular":"inside",
            "refractivity":["enabled":true,"depth":depth,"strength":0.10],
            "translucency-specializations":[
                specialization(nil,["enabled":true,"value":0.25]),
                specialization("dark",["enabled":true,"value":0.40]),
                specialization("tinted",["enabled":true,"value":0.60])
            ],
            "shadow-specializations":[
                specialization(nil,["kind":"neutral","opacity":0.28]),
                specialization("dark",["kind":"neutral","opacity":0.40]),
                specialization("tinted",["kind":"neutral","opacity":0.36])
            ]
        ])
    }
    if index==2 { groups.reverse() }
    let config:[String:Any]=[
        "features":["refractivity","specular-location"],
        "fill-specializations":[
            specialization(nil,color("extended-srgb:0.06,0.14,0.24,1.0")),
            specialization("dark",color("extended-srgb:0.025,0.05,0.09,1.0")),
            specialization("tinted",color("extended-srgb:0.12,0.12,0.12,1.0"))
        ],"groups":groups,"supported-platforms":["squares":["macOS"]]
    ]
    try JSONSerialization.data(withJSONObject:config,options:[.prettyPrinted,.sortedKeys]).write(to:directory.appendingPathComponent("AppIcon.icon/icon.json"))
    // At menu size, explicit optical boxes preserve whitespace around all
    // three parts. Do not maximize glyph scale inside the cloud frame.
    let menuShapes:[CGPath]
    if index==2 {
        let thinFrame=inset(opened,15),thinBounds=thinFrame.boundingBoxOfPath
        var frameFit=CGAffineTransform(translationX:iconBounds.minX,y:iconBounds.minY)
            .scaledBy(x:iconBounds.width/thinBounds.width,y:iconBounds.height/thinBounds.height)
            .translatedBy(x:-thinBounds.minX,y:-thinBounds.minY)
        let menuFrame=thinFrame.copy(using:&frameFit)!
        let source=expand(bolt,3),sourceBounds=source.boundingBoxOfPath
        var fit=CGAffineTransform(translationX:352,y:440)
            .scaledBy(x:132/sourceBounds.width,y:160/sourceBounds.height)
            .translatedBy(x:-sourceBounds.minX,y:-sourceBounds.minY)
        let menuBolt=source.copy(using:&fit)!
        let menuCursor=CGPath(roundedRect:CGRect(x:540,y:544,width:132,height:56),cornerWidth:28,cornerHeight:28,transform:nil)
        // About 1.84 pixels of geometric separation on a 22-pixel template.
        for (a,b) in [(menuFrame,menuBolt),(menuFrame,menuCursor),(menuBolt,menuCursor)] {
            precondition(a.intersection(expand(b,75)).isEmpty,"Menu glyph clearance fell below 75 source points")
        }
        menuShapes=[menuFrame,menuBolt,menuCursor]
    } else {
        var fit=CGAffineTransform(translationX:markBounds.midX,y:markBounds.midY)
            .scaledBy(x:1.10,y:1.10).translatedBy(x:-markBounds.midX,y:-markBounds.midY)
        let menuBolt=expand(bolt,7).copy(using:&fit)!
        let menuCursor=expand(cursor,7).copy(using:&fit)!
        menuShapes=[(index==0 ? rounded : flat).subtracting(menuBolt).subtracting(menuCursor)]
    }
    let bounds=iconBounds
    let rep=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:1024,pixelsHigh:1024,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
    NSGraphicsContext.saveGraphicsState();NSGraphicsContext.current=NSGraphicsContext(bitmapImageRep:rep)
    let context=NSGraphicsContext.current!.cgContext
    let scale=850/max(bounds.width,bounds.height)
    context.translateBy(x:512,y:512);context.scaleBy(x:scale,y:-scale);context.translateBy(x:-bounds.midX,y:-bounds.midY)
    context.setFillColor(NSColor.black.cgColor)
    for shape in menuShapes {context.addPath(shape);context.fillPath()}
    NSGraphicsContext.restoreGraphicsState()
    try rep.representation(using:.png,properties:[:])!.write(to:directory.appendingPathComponent("StatusBarIcon.png"))
    print(name)
}

for (index,name) in names.enumerated() {
    let legibility=Process()
    legibility.executableURL=URL(fileURLWithPath:"/usr/bin/env")
    legibility.arguments=["swift",root.appendingPathComponent("scripts/assets/check_menu_icon_legibility.swift").path,output.appendingPathComponent("\(name)/StatusBarIcon.png").path] + (index<2 ? ["--cutout"] : [])
    try legibility.run();legibility.waitUntilExit()
    if legibility.terminationStatus != 0 { exit(legibility.terminationStatus) }
}
