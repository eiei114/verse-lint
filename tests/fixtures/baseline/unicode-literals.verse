# Strings, comments and interpolation carry Unicode literals.
salute := class:
    Greet():void =
        Message := "こんにちは 世界 😀"
        Initial := 'e'
        Print(Message)
        Print("{Message} {Initial}")

# コメント内の日本語と　全角空白
spaced := class:
    Wide():void =
        Print("全角スペース　はリテラル内なので保護される")
