import SwiftUI
import UIKit

struct CustomTouchButton: Codable, Equatable, Identifiable {
    var id = UUID()
    var name = "New button"
    var bindings = [Int](repeating: 0, count: 4)
    var centre = TouchCentre(x: 0.5, y: 0.5)
    var size = TouchSize(width: 0.15, height: 0.12)
    var opacity = 0.7
    var inGame = true
    var inMenu = true
    var toggle = false
    func rect(in canvas: CGSize) -> CGRect {
        let unit = min(canvas.width, canvas.height)
        let width = min(canvas.width, unit * (size.valid ? size.width : 0.15))
        let height = min(canvas.height, unit * (size.valid ? size.height : 0.12))
        let c = centre.valid ? centre : TouchCentre(x:0.5,y:0.5)
        return CGRect(x:max(0,min(canvas.width-width,c.x*canvas.width-width/2)),
                      y:max(0,min(canvas.height-height,c.y*canvas.height-height/2)),width:width,height:height)
    }
    var keys: [Int] {
        bindings.prefix(4).reduce(into:[]) { result, code in
            if code != 0 && TouchBinding.choices.contains(where:{$0.code == code}) && !result.contains(code) {result.append(code)}
        }
    }
}
struct TouchBinding: Identifiable {
    let code: Int
    let title: String
    var id: Int { code }
    static let choices: [TouchBinding] = {
        var values = [TouchBinding(code:0,title:"None"), .init(code:-3,title:"Left mouse"), .init(code:-4,title:"Right mouse"),
                      .init(code:-6,title:"Middle mouse"), .init(code:-7,title:"Scroll up"), .init(code:-8,title:"Scroll down"), .init(code:-1,title:"Show/hide keyboard"), .init(code:-2,title:"Show/hide controls"), .init(code:-5,title:"Toggle menu cursor mode")]
        values += (65...90).map { .init(code:$0,title:String(UnicodeScalar($0)!)) }
        values += (48...57).map { .init(code:$0,title:String(UnicodeScalar($0)!)) }
        values += (0...9).map { .init(code:320+$0,title:"Keypad \($0)") }
        values += [(280,"Caps Lock"),(281,"Scroll Lock"),(282,"Num Lock"),(283,"Print Screen"),(284,"Pause"),
                   (330,"Keypad decimal"),(331,"Keypad divide"),(332,"Keypad multiply"),(333,"Keypad subtract"),(334,"Keypad add"),(335,"Keypad Enter"),(336,"Keypad equals")].map{.init(code:$0.0,title:$0.1)}
        values += (1...24).map { .init(code:289+$0,title:"F\($0)") }
        values += [(32,"Space"),(256,"Escape"),(257,"Enter"),(258,"Tab"),(259,"Backspace"),(260,"Insert"),(261,"Delete"),
                   (262,"Right arrow"),(263,"Left arrow"),(264,"Down arrow"),(265,"Up arrow"),(266,"Page up"),(267,"Page down"),
                   (268,"Home"),(269,"End"),(39,"Apostrophe"),(44,"Comma"),(45,"Minus"),(46,"Period"),(47,"Slash"),
                   (59,"Semicolon"),(61,"Equals"),(91,"Left bracket"),(92,"Backslash"),(93,"Right bracket"),(96,"Grave accent"),(340,"Left Shift"),(341,"Left Control"),(342,"Left Alt"),(343,"Left Command"),
                   (344,"Right Shift"),(345,"Right Control"),(346,"Right Alt"),(347,"Right Command")].map { .init(code:$0.0,title:$0.1) }
        return values
    }()
}

/// Shared holds across custom controls: one control releasing a chord must not
/// release a key that another control still owns.
final class CustomTouchHolds {
    private var owners: [UUID:[Int]] = [:]
    func set(_ id: UUID, keys: [Int], down: Bool) -> [(Int,Bool)] {
        let fresh = owners[id] == nil
        let old = owners[id] ?? []
        let before = Set(owners.values.flatMap{$0})
        if down { owners[id] = keys } else { owners.removeValue(forKey:id) }
        let after = Set(owners.values.flatMap{$0})
        return old.filter{!after.contains($0)}.map{($0,false)} + keys.filter{down && (fresh || !before.contains($0))}.map{($0,true)}
    }
    var modifiers: UIKeyModifierFlags {
        let keys=Set(owners.values.flatMap{$0});var result:UIKeyModifierFlags=[]
        if keys.contains(340)||keys.contains(344){result.insert(.shift)}
        if keys.contains(341)||keys.contains(345){result.insert(.control)}
        if keys.contains(342)||keys.contains(346){result.insert(.alternate)}
        if keys.contains(343)||keys.contains(347){result.insert(.command)}
        return result
    }
}
final class CustomTouchView: UIButton {
    var definition: CustomTouchButton
    var send: (UUID,[Int],Bool,UIKeyModifierFlags)->Void
    private var buttonHeld = false
    init(_ definition: CustomTouchButton, send: @escaping (UUID,[Int],Bool,UIKeyModifierFlags)->Void) {
        self.definition=definition; self.send=send;super.init(frame:.zero)
        setTitle(definition.name,for:.normal);setTitleColor(.systemYellow,for:.selected);backgroundColor=UIColor.black.withAlphaComponent(0.4)
        layer.borderColor=UIColor.white.withAlphaComponent(0.6).cgColor;layer.borderWidth=1;layer.cornerRadius=6
        titleLabel?.adjustsFontSizeToFitWidth=true;titleLabel?.minimumScaleFactor=0.4
        accessibilityIdentifier="custom-touch-\(definition.id)"
        addTarget(self,action:#selector(down(_:event:)),for:.touchDown)
        addTarget(self,action:#selector(up(_:event:)),for:.touchUpInside)
        addTarget(self,action:#selector(outside(_:event:)),for:.touchUpOutside)
        addTarget(self,action:#selector(cancelled),for:.touchCancel)
    }
    required init?(coder:NSCoder){fatalError("init(coder:) is unavailable")}
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        // A hardware pointer belongs to the game underneath the touch overlay.
        if event?.allTouches?.contains(where:{$0.type == .indirectPointer}) == true {return false}
        return super.point(inside:point,with:event)
    }
    @objc private func down(_ sender:UIButton,event:UIEvent?){
        if !definition.toggle {buttonHeld=true;send(definition.id,definition.keys,true,event?.modifierFlags ?? [])}
    }
    @objc private func up(_ sender:UIButton,event:UIEvent?){
        if definition.toggle {
            buttonHeld.toggle();isSelected=buttonHeld;send(definition.id,definition.keys,buttonHeld,event?.modifierFlags ?? [])
        } else {release(event?.modifierFlags ?? [])}
    }
    @objc private func outside(_ sender:UIButton,event:UIEvent?){if !definition.toggle {release(event?.modifierFlags ?? [])}}
    override func accessibilityActivate() -> Bool {
        if definition.toggle {buttonHeld.toggle();isSelected=buttonHeld;send(definition.id,definition.keys,buttonHeld,[])}
        else {send(definition.id,definition.keys,true,[]);send(definition.id,definition.keys,false,[])}
        return true
    }
    @objc private func cancelled(){release()}
    func release(_ flags:UIKeyModifierFlags=[]){if buttonHeld{buttonHeld=false;isSelected=false;send(definition.id,definition.keys,false,flags)}}
}

struct CustomTouchEditor: View {
    @Binding var button: CustomTouchButton
    var body: some View {
        Form {
            TextField("Button name",text:$button.name).accessibilityIdentifier("custom-button-name")
            Section("Bindings — press together") {
                ForEach(0..<4,id:\.self) { index in
                    Picker("Binding \(index+1)",selection:Binding(get:{button.bindings.indices.contains(index) ? button.bindings[index] : 0},set:{value in
                        while button.bindings.count<4 {button.bindings.append(0)};button.bindings[index]=value
                    })) {ForEach(TouchBinding.choices){Text($0.title).tag($0.code)}}.accessibilityIdentifier("custom-binding-\(index)")
                }
            }
            Section("Appearance") {
                Text("Width");Slider(value:$button.size.width,in:0.02...0.8)
                Text("Height");Slider(value:$button.size.height,in:0.02...0.8)
                Text("Opacity");Slider(value:$button.opacity,in:0.1...1)
            }
            Toggle("Show in game",isOn:$button.inGame)
            Toggle("Show in menus",isOn:$button.inMenu)
            Toggle("Toggle instead of hold",isOn:$button.toggle)
            Text("Hold is the default. Minecraft’s own toggle settings still apply.").font(.footnote)
        }.navigationTitle("Custom button")
    }
}
