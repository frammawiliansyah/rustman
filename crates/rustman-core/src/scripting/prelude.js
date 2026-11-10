(function (global) {
  var input = JSON.parse(__INPUT__);
  var logs = [];
  var results = [];

  function store(initial) {
    var data = Object.assign({}, initial);
    return {
      get: function (k) { return Object.prototype.hasOwnProperty.call(data, k) ? data[k] : undefined; },
      set: function (k, v) { data[String(k)] = (v === undefined || v === null) ? "" : (typeof v === "object" ? JSON.stringify(v) : String(v)); },
      unset: function (k) { delete data[k]; },
      has: function (k) { return Object.prototype.hasOwnProperty.call(data, k); },
      clear: function () { Object.keys(data).forEach(function (k) { delete data[k]; }); },
      toObject: function () { return Object.assign({}, data); },
      __data: data
    };
  }
  var env = store(input.environment);
  var vars = store(input.variables);
  var globals = store(input.globals);
  var iterData = store(input.iterationData);
  var iterationData = {
    get: iterData.get, has: iterData.has, toObject: iterData.toObject,
    toJSON: iterData.toObject
  };
  var pmVars = Object.assign({}, vars, {
    get: function (k) {
      if (iterData.has(k)) return iterData.get(k);
      return vars.has(k) ? vars.get(k) : env.get(k);
    }
  });

  function AssertionError(msg) { this.name = "AssertionError"; this.message = msg; }
  AssertionError.prototype = Object.create(Error.prototype);

  function show(v) { try { return typeof v === "string" ? JSON.stringify(v) : (v === undefined ? "undefined" : JSON.stringify(v)); } catch (e) { return String(v); } }
  function deepEqual(a, b) {
    if (a === b) return true;
    if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) {
      return a !== a && b !== b;
    }
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    var ka = Object.keys(a), kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    for (var i = 0; i < ka.length; i++) {
      if (!Object.prototype.hasOwnProperty.call(b, ka[i]) || !deepEqual(a[ka[i]], b[ka[i]])) return false;
    }
    return true;
  }
  function typeName(v) {
    if (v === null) return "null";
    if (Array.isArray(v)) return "array";
    return typeof v;
  }

  function expect(val) {
    var negate = false;
    var deep = false;
    var a = {};
    function check(ok, msg, negMsg) {
      if (negate ? ok : !ok) throw new AssertionError(negate ? negMsg : msg);
    }
    var chain = ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "but", "does", "still", "also"];
    chain.forEach(function (w) { Object.defineProperty(a, w, { get: function () { return a; } }); });
    Object.defineProperty(a, "not", { get: function () { negate = !negate; return a; } });
    Object.defineProperty(a, "deep", { get: function () { deep = true; return a; } });
    function flag(name, fn) { Object.defineProperty(a, name, { get: function () { fn(); return a; } }); }
    flag("ok", function () { check(!!val, "expected " + show(val) + " to be truthy", "expected " + show(val) + " to be falsy"); });
    flag("true", function () { check(val === true, "expected " + show(val) + " to be true", "expected " + show(val) + " not to be true"); });
    flag("false", function () { check(val === false, "expected " + show(val) + " to be false", "expected " + show(val) + " not to be false"); });
    flag("null", function () { check(val === null, "expected " + show(val) + " to be null", "expected value not to be null"); });
    flag("undefined", function () { check(val === undefined, "expected " + show(val) + " to be undefined", "expected value not to be undefined"); });
    flag("exist", function () { check(val !== null && val !== undefined, "expected value to exist", "expected value not to exist"); });
    flag("empty", function () {
      var e = (typeof val === "string" || Array.isArray(val)) ? val.length === 0 : (val && typeof val === "object" ? Object.keys(val).length === 0 : false);
      check(e, "expected " + show(val) + " to be empty", "expected " + show(val) + " not to be empty");
    });
    function eq(expected) {
      var ok = deep ? deepEqual(val, expected) : val === expected;
      check(ok, "expected " + show(val) + " to equal " + show(expected), "expected " + show(val) + " not to equal " + show(expected));
      return a;
    }
    ["equal", "equals", "eq"].forEach(function (n) { a[n] = eq; });
    ["eql", "eqls"].forEach(function (n) { a[n] = function (x) { var d = deep; deep = true; try { return eq(x); } finally { deep = d; } }; });
    a.above = a.gt = a.greaterThan = function (n) { check(val > n, "expected " + show(val) + " to be above " + n, "expected " + show(val) + " not to be above " + n); return a; };
    a.below = a.lt = a.lessThan = function (n) { check(val < n, "expected " + show(val) + " to be below " + n, "expected " + show(val) + " not to be below " + n); return a; };
    a.least = a.gte = function (n) { check(val >= n, "expected " + show(val) + " to be at least " + n, "expected " + show(val) + " to be below " + n); return a; };
    a.most = a.lte = function (n) { check(val <= n, "expected " + show(val) + " to be at most " + n, "expected " + show(val) + " to be above " + n); return a; };
    a.within = function (lo, hi) { check(val >= lo && val <= hi, "expected " + show(val) + " to be within " + lo + ".." + hi, "expected " + show(val) + " not to be within " + lo + ".." + hi); return a; };
    a.a = a.an = function (t) { var ok = typeName(val) === String(t).toLowerCase(); check(ok, "expected " + show(val) + " to be a " + t, "expected " + show(val) + " not to be a " + t); return a; };
    a.include = a.includes = a.contain = a.contains = function (x) {
      var ok;
      if (typeof val === "string") ok = val.indexOf(x) !== -1;
      else if (Array.isArray(val)) ok = val.some(function (i) { return deep ? deepEqual(i, x) : i === x; });
      else if (val && typeof val === "object" && x && typeof x === "object") ok = Object.keys(x).every(function (k) { return deepEqual(val[k], x[k]); });
      else ok = false;
      check(ok, "expected " + show(val) + " to include " + show(x), "expected " + show(val) + " not to include " + show(x));
      return a;
    };
    a.property = function (name, v) {
      var has = val !== null && val !== undefined && Object.prototype.hasOwnProperty.call(Object(val), name);
      var ok = has && (arguments.length < 2 || (deep ? deepEqual(val[name], v) : val[name] === v));
      check(ok, "expected " + show(val) + " to have property " + show(name) + (arguments.length > 1 ? " of " + show(v) : ""), "expected " + show(val) + " not to have property " + show(name));
      return a;
    };
    a.lengthOf = a.length = function (n) {
      var l = val === null || val === undefined ? undefined : val.length;
      check(l === n, "expected length " + l + " to equal " + n, "expected length not to equal " + n);
      return a;
    };
    a.match = function (re) { check(re.test(val), "expected " + show(val) + " to match " + re, "expected " + show(val) + " not to match " + re); return a; };
    a.oneOf = function (list) { check(list.indexOf(val) !== -1, "expected " + show(val) + " to be one of " + show(list), "expected " + show(val) + " not to be one of " + show(list)); return a; };
    a.keys = function () {
      var want = Array.prototype.slice.call(arguments).reduce(function (acc, k) { return acc.concat(k); }, []);
      var have = Object.keys(val || {});
      var ok = want.every(function (k) { return have.indexOf(k) !== -1; });
      check(ok, "expected " + show(have) + " to have keys " + show(want), "expected " + show(have) + " not to have keys " + show(want));
      return a;
    };
    return a;
  }

  var response = null;
  if (input.response) {
    var r = input.response;
    var lower = {};
    r.headers.forEach(function (h) { lower[h[0].toLowerCase()] = h[1]; });
    function assertStatus(expected) {
      var ok = typeof expected === "number" ? r.code === expected : r.status === expected;
      if (!ok) throw new AssertionError("expected response to have status " + show(expected) + " but got " + r.code + " " + show(r.status));
    }
    response = {
      code: r.code,
      status: r.status,
      responseTime: r.responseTime,
      text: function () { return r.body; },
      json: function () { return JSON.parse(r.body); },
      headers: {
        get: function (k) { return lower[String(k).toLowerCase()]; },
        has: function (k) { return Object.prototype.hasOwnProperty.call(lower, String(k).toLowerCase()); },
        toObject: function () { return Object.assign({}, lower); }
      },
      to: {
        have: {
          status: assertStatus,
          header: function (k, v) {
            var got = lower[String(k).toLowerCase()];
            if (got === undefined || (arguments.length > 1 && got !== v)) throw new AssertionError("expected response to have header " + show(k));
          },
          body: function (v) { if (r.body !== v) throw new AssertionError("expected response body to equal " + show(v)); }
        },
        be: {
          get ok() { if (r.code < 200 || r.code >= 300) throw new AssertionError("expected response to be 2xx but got " + r.code); return true; }
        }
      }
    };
  }

  var pm = {
    environment: env,
    variables: pmVars,
    globals: globals,
    iterationData: iterationData,
    response: response,
    info: { eventName: input.event },
    expect: expect,
    test: function (name, fn) {
      try {
        if (typeof fn === "function") fn();
        results.push({ name: String(name), passed: true, message: null });
      } catch (e) {
        results.push({ name: String(name), passed: false, message: (e && e.message) ? String(e.message) : String(e) });
      }
    }
  };
  pm.sendRequest = function (req, callback) {
    var spec = typeof req === "string" ? { url: req } : (req || {});
    var headers = [];
    var h = spec.header || spec.headers;
    if (Array.isArray(h)) {
      h.forEach(function (p) { if (p && p.key !== undefined) headers.push([String(p.key), String(p.value)]); });
    } else if (h && typeof h === "object") {
      Object.keys(h).forEach(function (k) { headers.push([k, String(h[k])]); });
    }
    var body = spec.body;
    if (body && typeof body === "object") {
      body = body.mode === "raw" ? body.raw : JSON.stringify(body);
    }
    var raw = JSON.parse(__http(JSON.stringify({
      url: String(spec.url),
      method: String(spec.method || "GET"),
      headers: headers,
      body: (body === undefined || body === null) ? null : String(body)
    })));
    var err = null, res = null;
    if (raw.error) {
      err = new Error(raw.error);
    } else {
      var hmap = {};
      raw.headers.forEach(function (p) {
        var k = p[0].toLowerCase();
        hmap[k] = Object.prototype.hasOwnProperty.call(hmap, k) ? hmap[k] + ", " + p[1] : p[1];
      });
      res = {
        code: raw.code,
        status: raw.status,
        headers: hmap,
        text: function () { return raw.body; },
        json: function () { return JSON.parse(raw.body); }
      };
    }
    if (typeof callback === "function") callback(err, res);
  };
  global.pm = pm;
  function wordArray(str) {
    return { toString: function (enc) { return enc && enc.stringify ? enc.stringify(str) : str; } };
  }
  global.CryptoJS = {
    MD5: function (s) { var h = __md5(String(s)); return { toString: function () { return h; } }; },
    SHA256: function (s) { var h = __sha256(String(s)); return { toString: function () { return h; } }; },
    HmacSHA256: function (m, k) { var h = __hmac_sha256(String(m), String(k)); return { toString: function () { return h; } }; },
    enc: {
      Base64: {
        stringify: function (v) { return __b64_encode(String(v)); },
        parse: function (b64) { return wordArray(__b64_decode(String(b64))); }
      },
      Utf8: { stringify: function (v) { return String(v); }, parse: function (s) { return wordArray(String(s)); } }
    }
  };
  global.console = {
    log: function () { logs.push(Array.prototype.slice.call(arguments).map(function (x) { return typeof x === "string" ? x : show(x); }).join(" ")); }
  };
  global.console.info = global.console.warn = global.console.error = global.console.debug = global.console.log;
  global.__collect = function () {
    return JSON.stringify({ environment: env.__data, variables: vars.__data, globals: globals.__data, results: results, logs: logs });
  };
})(globalThis);
