import Foundation

/// Bounded, native presentation of the block forms used by remote.js. Inline
/// emphasis and links are rendered by AttributedString; HTML is always text.
public enum MarkdownBlock: Equatable, Sendable {
    case paragraph(String), heading(Int, String), code(String), quote(String)
    case table([[String]]), image(String, String)
}

public enum RemoteMarkdown {
    public static func blocks(_ source: String) -> [MarkdownBlock] {
        let lines = source.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        var result: [MarkdownBlock] = []
        var paragraph: [String] = []
        func flush() {
            if !paragraph.isEmpty { result.append(.paragraph(paragraph.joined(separator: "\n"))); paragraph = [] }
        }
        var i = 0
        while i < lines.count {
            let line = lines[i]
            let trim = line.trimmingCharacters(in: .whitespaces)
            if trim.hasPrefix("```") || trim.hasPrefix("~~~") {
                flush()
                let character = trim.first!
                let fence = String(trim.prefix { $0 == character })
                var code: [String] = []
                i += 1
                while i < lines.count && !lines[i].trimmingCharacters(in: .whitespaces).hasPrefix(fence) {
                    code.append(lines[i]); i += 1
                }
                result.append(.code(code.joined(separator: "\n")))
            } else if i + 1 < lines.count && line.contains("|") && tableRule(lines[i + 1]) {
                flush()
                var rows = [cells(line)]
                i += 2
                while i < lines.count && lines[i].contains("|") && !lines[i].isEmpty {
                    rows.append(cells(lines[i])); i += 1
                }
                result.append(.table(rows)); continue
            } else if trim.isEmpty { flush() }
            else if trim.hasPrefix("#"), let space = trim.firstIndex(of: " "),
                    trim[..<space].allSatisfy({ $0 == "#" }), trim.distance(from: trim.startIndex, to: space) <= 6 {
                flush()
                result.append(.heading(trim.distance(from: trim.startIndex, to: space), String(trim[trim.index(after: space)...])))
            } else if trim.hasPrefix(">") {
                flush(); result.append(.quote(String(trim.dropFirst()).trimmingCharacters(in: .whitespaces)))
            } else {
                let pattern = #"!\[([^\]]*)\]\(\s*(?:<([^>]+)>|([^)\s]+))(?:\s+"[^"]*")?\s*\)"#
                let regex = try! NSRegularExpression(pattern: pattern)
                let matches = regex.matches(in: line, range: NSRange(line.startIndex..., in: line))
                if matches.isEmpty { paragraph.append(line) }
                else {
                    flush()
                    let ns = line as NSString
                    var offset = 0
                    for match in matches {
                        let prefix = ns.substring(with: NSRange(location: offset, length: match.range.location - offset))
                        if !prefix.isEmpty { result.append(.paragraph(prefix)) }
                        let target = match.range(at: 2).location == NSNotFound ? match.range(at: 3) : match.range(at: 2)
                        result.append(.image(ns.substring(with: target), ns.substring(with: match.range(at: 1))))
                        offset = NSMaxRange(match.range)
                    }
                    let suffix = ns.substring(from: offset)
                    if !suffix.isEmpty { result.append(.paragraph(suffix)) }
                }
            }
            i += 1
        }
        flush()
        return result
    }
    private static func cells(_ row: String) -> [String] {
        var text = row.trimmingCharacters(in: .whitespaces)
        if text.hasPrefix("|") { text.removeFirst() }
        if text.hasSuffix("|") { text.removeLast() }
        return text.replacingOccurrences(of: "\\|", with: "\u{0}").components(separatedBy: "|")
            .map { $0.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "\u{0}", with: "|") }
    }
    private static func tableRule(_ line: String) -> Bool {
        line.contains("|") && cells(line).allSatisfy { cell in
            let value = cell.replacingOccurrences(of: ":", with: "")
            return !value.isEmpty && value.allSatisfy { $0 == "-" }
        }
    }
    public static func localPath(_ reference: String, directory: String = ".") -> String? {
        let reference = reference.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let text = reference.removingPercentEncoding, !text.isEmpty, !text.hasPrefix("//"),
              !text.hasPrefix("#"), URLComponents(string: text)?.scheme == nil else { return nil }
        if text.hasPrefix("/") { return text }
        var parts = directory == "." ? [] : directory.components(separatedBy: "/")
        for part in text.components(separatedBy: "/") {
            if part == ".." { if !parts.isEmpty { parts.removeLast() } }
            else if !part.isEmpty && part != "." { parts.append(part) }
        }
        return parts.isEmpty ? "." : parts.joined(separator: "/")
    }
}
