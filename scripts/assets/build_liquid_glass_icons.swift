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
// Reference-led cloud: rounded left mass, a flat base, and a regular
// 64-point stair stepping down toward the right. No photographic tracing.
let cloudBody=CGMutablePath()
cloudBody.move(to:CGPoint(x:444,y:284))
cloudBody.addCurve(to:CGPoint(x:268,y:460),control1:CGPoint(x:347,y:284),control2:CGPoint(x:268,y:363))
cloudBody.addCurve(to:CGPoint(x:170,y:579),control1:CGPoint(x:212,y:468),control2:CGPoint(x:170,y:518))
cloudBody.addCurve(to:CGPoint(x:294,y:700),control1:CGPoint(x:170,y:648),control2:CGPoint(x:225,y:700))
cloudBody.addLine(to:CGPoint(x:746,y:700))
cloudBody.addCurve(to:CGPoint(x:828,y:618),control1:CGPoint(x:792,y:700),control2:CGPoint(x:828,y:664))
cloudBody.addCurve(to:CGPoint(x:764,y:538),control1:CGPoint(x:828,y:579),control2:CGPoint(x:801,y:546))
cloudBody.addLine(to:CGPoint(x:700,y:538));cloudBody.addLine(to:CGPoint(x:700,y:476))
cloudBody.addLine(to:CGPoint(x:636,y:476));cloudBody.addLine(to:CGPoint(x:636,y:412))
cloudBody.addLine(to:CGPoint(x:572,y:412));cloudBody.addLine(to:CGPoint(x:572,y:348))
cloudBody.addLine(to:CGPoint(x:508,y:348));cloudBody.addLine(to:CGPoint(x:508,y:284))
cloudBody.closeSubpath()
let round=cloudBody as CGPath
// Dense attached tiles turn into sparse, pale detached pixels on one grid.
// Overlays on the cloud keep the transition continuous rather than cut away.
let pixelCells:[(Int,Int,Double)]=[
    (0,0,0.18),(1,0,0.43),(3,0,0.82),(6,0,0.90),
    (0,1,0.10),(1,1,0.25),(2,1,0.55),(4,1,0.69),
    (1,2,0.12),(2,2,0.32),(3,2,0.62),(5,2,0.84),
    (2,3,0.14),(3,3,0.38),(4,3,0.57)]
let pixels:[CGPath]=pixelCells.map {col,row,_ in
    CGPath(rect:CGRect(x:444+col*64,y:284+row*64,width:64,height:64),transform:nil)
}

let capsule=CGPath(roundedRect:CGRect(x:140,y:362,width:744,height:374),cornerWidth:172,cornerHeight:172,transform:nil)
let flat=closeCorners(capsule.union(circle(512,385,174)),16)
let rounded=round
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
    var pixelFit=CGAffineTransform(translationX:markBounds.midX-12,y:markBounds.midY+33).scaledBy(x:0.88,y:0.72).translatedBy(x:-markBounds.midX,y:-markBounds.midY)
    let pixelBolt=bolt.copy(using:&pixelFit)!,pixelCursor=cursor.copy(using:&pixelFit)!
    // Approved Dock contour: soften the left shoulder and use a circular right cap.
    // Keep the menu bar optical geometry independent at its small display size.
    let dockCloud=CGMutablePath()
    dockCloud.move(to:CGPoint(x:508,y:348))
    for point in [CGPoint(x:572,y:348),CGPoint(x:572,y:412),CGPoint(x:636,y:412),CGPoint(x:636,y:476),CGPoint(x:700,y:476),CGPoint(x:700,y:540),CGPoint(x:756,y:540)] { dockCloud.addLine(to:point) }
    dockCloud.addCurve(to:CGPoint(x:836,y:620),control1:CGPoint(x:800.183,y:540),control2:CGPoint(x:836,y:575.817))
    dockCloud.addCurve(to:CGPoint(x:756,y:700),control1:CGPoint(x:836,y:664.183),control2:CGPoint(x:800.183,y:700))
    dockCloud.addLine(to:CGPoint(x:294,y:700))
    dockCloud.addCurve(to:CGPoint(x:173,y:579),control1:CGPoint(x:227.174,y:700),control2:CGPoint(x:173,y:645.826))
    dockCloud.addCurve(to:CGPoint(x:258,y:466),control1:CGPoint(x:173,y:525),control2:CGPoint(x:209,y:480))
    dockCloud.addCurve(to:CGPoint(x:269,y:452),control1:CGPoint(x:264,y:464),control2:CGPoint(x:268,y:459))
    dockCloud.addCurve(to:CGPoint(x:444,y:284),control1:CGPoint(x:273,y:358),control2:CGPoint(x:349,y:284))
    dockCloud.addLine(to:CGPoint(x:508,y:284));dockCloud.closeSubpath()
    let shapes:[CGPath]=index==0 ? [dockCloud.subtracting(pixelBolt).subtracting(pixelCursor)] + pixels : (index==2 ? [opened,insetBolt,insetCursor] : [flat.subtracting(bolt).subtracting(cursor)])
    let iconBounds=(index==0 ? rounded : flat).boundingBoxOfPath
    let dockOffsetX=(512-iconBounds.midX)*0.95
    let dockOffsetY=(512-iconBounds.midY)*0.95
    var groups=[[String:Any]]()
    for (i,shape) in shapes.enumerated() {
        let layerName=index==0 ? (i==0 ? "Cloud with cutouts" : "Pixel \(i)") : index==2 ? ["Cloud frame","Lightning","Cursor"][i] : "Cloud with inset mark"
        let filename="shape-\(i).svg"
        var artwork=svg(shape)
        if index==0 {
            // Enlarge the complete mark together without changing its glass material.
            artwork=artwork.replacingOccurrences(of:"<path ",with:"<g transform=\"matrix(1.18 0 0 1.18 -114.58 -68.56)\"><path ")
                .replacingOccurrences(of:"</svg>",with:"</g></svg>")
        }
        try artwork.write(to:assets.appendingPathComponent(filename),atomically:true,encoding:.utf8)
        let fade=index==0 && i>0 ? pixelCells[i-1].2 : 0
        let lightFill=index==0 ? String(format:"extended-srgb:%.3f,%.3f,%.3f,1.0",fade*0.78,0.64+fade*0.32,0.86+fade*0.13) : "extended-srgb:0.76,0.84,0.92,1.0"
        let darkFill=index==0 ? String(format:"extended-srgb:%.3f,%.3f,%.3f,0.9",0.08+fade*0.45,0.40+fade*0.43,0.62+fade*0.35) : "extended-srgb:0.48,0.57,0.67,0.88"
        let monoFill=index==0 ? String(format:"extended-srgb:%.3f,%.3f,%.3f,0.68",0.60+fade*0.32,0.60+fade*0.32,0.60+fade*0.32) : "extended-srgb:0.66,0.66,0.66,0.68"
        let layer:[String:Any]=[
            "name":layerName,"image-name":filename,
            "position":["scale":0.95,"translation-in-points":[dockOffsetX,dockOffsetY]],
            "fill-specializations":[
                specialization(nil,color(lightFill)),
                specialization("dark",color(darkFill)),
                specialization("tinted",color(monoFill))
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
    if index==0 {
        var pixelGroup=groups[1]
        pixelGroup["name"]="Dispersing pixels"
        pixelGroup["specular"]="inside"
        pixelGroup["refractivity"]=["enabled":true,"depth":0.16,"strength":0.10]
        pixelGroup["shadow-specializations"]=[
            specialization(nil,["kind":"neutral","opacity":0.22]),
            specialization("dark",["kind":"neutral","opacity":0.30]),
            specialization("tinted",["kind":"neutral","opacity":0.26])
        ]

        pixelGroup["layers"]=groups.dropFirst().flatMap { $0["layers"] as! [[String:Any]] }
        groups=[pixelGroup,groups[0]]
    }
    if index==2 { groups.reverse() }
    let config:[String:Any]=[
        "features":["refractivity","specular-location"],
        "fill-specializations":[
            specialization(nil,index==0 ? ["solid":"extended-srgb:1.0,1.0,1.0,1.0"] : color("extended-srgb:0.06,0.14,0.24,1.0")),
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
    } else if index==0 {
        // Use the same 15 cells as Dock. Only paint and small-size glyph fit differ.
        let b=bolt.boundingBoxOfPath
        var fit=CGAffineTransform(translationX:330,y:460).scaledBy(x:145/b.width,y:190/b.height).translatedBy(x:-b.minX,y:-b.minY)
        let menuBolt=bolt.copy(using:&fit)!
        let menuCursor=CGPath(roundedRect:CGRect(x:530,y:570,width:140,height:80),cornerWidth:40,cornerHeight:40,transform:nil)
        menuShapes=[pixels.reduce(rounded) { $0.union($1) }.subtracting(menuBolt).subtracting(menuCursor)]

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
    // Export menu-sized representations directly instead of resampling the
    // 1024px image at runtime. No detached cell is omitted or repositioned.
    for edge in [22,44] {
        let small=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:edge,pixelsHigh:edge,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bytesPerRow:0,bitsPerPixel:0)!
        NSGraphicsContext.saveGraphicsState();NSGraphicsContext.current=NSGraphicsContext(bitmapImageRep:small)
        let c=NSGraphicsContext.current!.cgContext
        let factor=CGFloat(edge)/1024,fitScale=scale*factor
        c.saveGState();c.translateBy(x:CGFloat(edge)/2,y:CGFloat(edge)/2)
        c.scaleBy(x:fitScale,y:-fitScale);c.translateBy(x:-bounds.midX,y:-bounds.midY)
        c.setFillColor(NSColor.black.cgColor)
        for shape in menuShapes { c.addPath(shape);c.fillPath() }
        c.restoreGState()
        NSGraphicsContext.restoreGraphicsState()
        small.size=NSSize(width:22,height:22)
        try small.representation(using:.png,properties:[:])!.write(to:directory.appendingPathComponent(edge==22 ? "StatusBarIcon-22.png" : "StatusBarIcon-22@2x.png"))
    }
    print(name)
}

for (index,name) in names.enumerated() {
    let legibility=Process()
    legibility.executableURL=URL(fileURLWithPath:"/usr/bin/env")
    legibility.arguments=["swift",root.appendingPathComponent("scripts/assets/check_menu_icon_legibility.swift").path,output.appendingPathComponent("\(name)/StatusBarIcon.png").path] + (index==0 ? ["--cutout","--pixels"] : (index==1 ? ["--cutout"] : []))
    try legibility.run();legibility.waitUntilExit()
    if legibility.terminationStatus != 0 { exit(legibility.terminationStatus) }
}
