"""Parse complete SNBT values without PR-specific capture imports."""
import math
import re


class Snbt:
    def __init__(self, text):
        if not isinstance(text, str) or len(text) > 1_000_000:
            raise ValueError('The SNBT input is invalid or too large.')
        self.text, self.position, self.depth = text, 0, 0

    def whitespace(self):
        while self.position < len(self.text) and self.text[self.position].isspace():
            self.position += 1

    def compound_key(self, allow_resource_keys=False):
        if self.position >= len(self.text):
            raise ValueError('The SNBT compound is incomplete.')
        if self.text[self.position] in ('"', "'"):
            return self.value()
        if allow_resource_keys:
            remaining = self.text[self.position:]
            match = re.match(r'([a-z0-9_.-]+:[a-z0-9/._-]+)(?=: )', remaining)
            if match:
                self.position += len(match[1])
                return match[1]
            candidate = re.split(r'[,{}]', remaining, maxsplit=1)[0]
            if candidate.count(':') > 1 and not remaining.split(':',1)[1].startswith(' '):
                raise ValueError('The component key is ambiguous or malformed.')
        key = self.token(':')
        if not re.fullmatch(r'[A-Za-z0-9_.+-]+', key):
            raise ValueError('The unquoted SNBT key differs.')
        return key

    def value(self, allow_resource_keys=False):
        self.depth += 1
        try:
            if self.depth > 64:
                raise ValueError('The SNBT nesting exceeds 64 levels.')
            return self._value(allow_resource_keys)
        finally:
            self.depth -= 1

    def _value(self, allow_resource_keys=False):
        self.whitespace()
        if self.position >= len(self.text):
            raise ValueError('The SNBT value is incomplete.')
        character = self.text[self.position]
        if character in ('"', "'"):
            quote = character
            self.position += 1
            output = ''
            while self.position < len(self.text):
                character = self.text[self.position]
                self.position += 1
                if character == quote:
                    return output
                if character == '\\':
                    if self.position >= len(self.text):
                        break
                    character = self.text[self.position]
                    self.position += 1
                    if character not in (quote, '\\'):
                        raise ValueError('The SNBT string escape differs.')
                output += character
            raise ValueError('The SNBT string is incomplete.')
        if character == '{':
            self.position += 1
            result = {}
            while True:
                self.whitespace()
                if self.text[self.position:self.position+1] == '}':
                    self.position += 1
                    return result
                key = self.compound_key(allow_resource_keys)
                self.whitespace()
                self.require(':')
                if not isinstance(key, str) or not key or key in result:
                    raise ValueError('The SNBT key is empty, invalid, or repeated.')
                result[key] = self.value(allow_resource_keys=key=='components')
                self.whitespace()
                if self.text[self.position:self.position+1] == '}':
                    continue
                self.require(',')
        if character == '[':
            self.position += 1
            self.whitespace()
            array_type = None
            if self.text[self.position:self.position+2] in ('I;', 'B;', 'L;'):
                array_type = self.text[self.position]
                self.position += 2
            array_start = self.position
            result = []
            while True:
                self.whitespace()
                if self.text[self.position:self.position+1] == ']':
                    if array_type:
                        bits = {'B':8,'I':32,'L':64}[array_type]
                        suffix = {'B':'[bB]?', 'I':'', 'L':'[lL]?'}[array_type]
                        parts = self.text[array_start:self.position].split(',') if result else []
                        if len(parts) != len(result) or any(not re.fullmatch(r'[-+]?\d+'+suffix, raw.strip()) for raw in parts) or any(type(value) is not int or not -(2**(bits-1)) <= value < 2**(bits-1) for value in result):
                            raise ValueError('The typed SNBT array element differs.')
                    self.position += 1
                    return result
                result.append(self.value())
                self.whitespace()
                if self.text[self.position:self.position+1] == ']':
                    continue
                self.require(',')
        token = self.token(',]}')
        if token in ('true', 'false'):
            return token == 'true'
        number = re.fullmatch(r'([-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?)[bBsSlLfFdD]?', token)
        if number:
            raw = number.group(1)
            value = float(raw) if any(c in raw for c in '.eE') else int(raw)
            if isinstance(value, float) and not math.isfinite(value):
                raise ValueError('The SNBT number is not finite.')
            return value
        if not token or token == '...' or not re.fullmatch(r'[A-Za-z0-9_./:+-]+', token) or re.fullmatch(r'[-+]?(?:NaN|Infinity)[fFdD]?', token, re.I):
            raise ValueError('The SNBT value is absent or truncated.')
        return token

    def token(self, stops):
        start = self.position
        while self.position < len(self.text) and self.text[self.position] not in stops:
            self.position += 1
        return self.text[start:self.position].strip()

    def require(self, character):
        if self.text[self.position:self.position+1] != character:
            raise ValueError('The SNBT separator differs.')
        self.position += 1



def parse_snbt(text, *, component_context=False):
    parser = Snbt(text)
    value = parser.value(allow_resource_keys=component_context)
    parser.whitespace()
    if parser.position != len(parser.text):
        raise ValueError('The SNBT reply has extra or truncated text.')
    return value
