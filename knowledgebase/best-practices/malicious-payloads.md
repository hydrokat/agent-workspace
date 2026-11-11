# Malicious Payload Reference

Use these payload categories when writing sad-path tests. Apply to every
user-controlled input: body fields, query params, route params, headers.

## Cross-Site Scripting (XSS)

```
<script>alert(1)</script>
<img src=x onerror=alert(1)>
<a href="javascript:alert(1)">click</a>
<svg onload=alert(1)>
{{constructor.constructor('alert(1)')()}}
javascript:alert(1)
```

Expectation: System must reject or safely encode. Never render unescaped HTML.

## SQL Injection

```
' OR 1=1 --
' UNION SELECT * FROM users --
1; DROP TABLE users --
admin'--
```

Expectation: Parameterized queries or ORM must prevent injection. The system
must not return unexpected data or crash.

## NoSQL Injection

```
{ "$gt": "" }
{ "$ne": null }
{ "$where": "this.password.startsWith('a')" }
```

Expectation: The system must validate or sanitize operators in query input.

## Command Injection

```
; ls
| cat /etc/passwd
$(cat /etc/passwd)
`cat /etc/passwd`
&& whoami
```

Expectation: System must not pass user input to shell commands. If unavoidable,
strictly validate against an allowlist.

## Path Traversal

```
../../../etc/passwd
..\..\..\windows\win.ini
%2e%2e%2f%2e%2e%2fetc/passwd
....//....//....//etc/passwd
```

Expectation: System must resolve paths against a safe root and reject traversal
patterns.

## Template Injection (SSTI)

```
{{7*7}}
{{config}}
{{self.__class__.__mro__}}
{{7*'7'}}
```

Expectation: System must not evaluate user input as a template expression.

## Mass Assignment / Prototype Pollution

```
{"role": "admin", "is_admin": true}
{"__proto__": {"admin": true}}
{"constructor": {"prototype": {"admin": true}}}
```

Expectation: System must use allowlisted fields for deserialization. Never
accept arbitrary object keys from user input.

## Oversized / Boundary Payloads

```
A 10KB+ string in a 100-character-max field
A JSON payload exceeding the max request size
An array with 100_000+ elements
A deeply nested JSON object (100+ levels)
```

Expectation: System must enforce size limits early and return a 413 or 422.

## Encoding Bypass Attempts

```
%3Cscript%3Ealert(1)%3C/script%3E   (URL encoding)
&#60;script&#62;alert(1)&#60;/script&#62; (HTML entities)
\x3Cscript\x3E                        (hex entities)
\u003Cscript\u003E                     (unicode escapes)
```

Expectation: System must normalize input before validation, not just block
literal patterns.