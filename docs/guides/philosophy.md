# Lithair Philosophy: The Origin Story

By Yoan Roblet

> "I just wanted to build a simple website, and I was already exhausted by
> the architecture part, especially the database."

## 🎯 The Frustration That Started It All

My name is **Yoan Roblet**, and Lithair was born from a simple frustration that
many web developers know well.

Picture this: You have a brilliant idea for a web application. Maybe it's an
e-commerce site, a blog, a social platform—doesn't matter. You're excited,
motivated, ready to build something amazing.

Then reality hits.

Before you can write a single line of business logic, you're drowning in
architectural decisions:

- **Frontend**: React or Vue? Webpack or Vite? TypeScript or JavaScript?
- **Backend**: Node.js, Python, Go, or Rust? Express, FastAPI, or Gin?
- **Database**: PostgreSQL, MySQL, or MongoDB? How do I handle migrations?
- **Infrastructure**: Docker? Kubernetes? AWS or Google Cloud?
- **Deployment**: CI/CD pipelines, load balancers, monitoring...

**For what? Three simple tables!**

I found myself spending 80% of my time on infrastructure and only 20% on the
actual product. The thing I was passionate about—the business logic, the user
experience, the problem I wanted to solve—became an afterthought.

## 💡 The Epiphany

One day, after setting up yet another PostgreSQL connection pool for a simple
CRUD application, I had an epiphany:

**"What if we didn't need all this complexity?"**

## 🤝 A Note on Respect

**Let me be crystal clear**: I'm not criticizing existing tools. I use
PostgreSQL, React, Kubernetes, and all these amazing technologies every day.
They are incredible achievements that have enabled the modern web.

This isn't about what's wrong with current tools—it's about **what's possible
with a different approach**.

## 🔮 A Modern Vision of Computing

My insight came from observing how computing has evolved:

### The Old Paradigm (1990s-2010s)

- **Scarcity mindset**: CPU, memory, and network were expensive
- **Separation of concerns**: Database server, application server, web server
- **SQL as the universal interface**: Connect, query, disconnect
- **Network as a necessary evil**: Accept the latency, manage the connections

### The New Reality (2020s+)

- **Abundance mindset**: CPU and memory are cheap, networks are fast
- **Co-location possibilities**: Everything can run on the same node
- **Delta synchronization**: Share changes, not full state
- **Network as an optimization**: Use it for coordination, not every operation

### Learning from Distributed Storage

Systems like **Ceph** have already proven this concept in storage:

- Instead of connecting to a central storage server, each node IS storage
- Instead of network I/O for every read, data is local
- Instead of bottlenecks, you get linear scaling
- **Result**: Much higher IOPS and throughput

Lithair applies the same principle to databases:

- Instead of connecting to a database server, each node IS the database
- Instead of network queries, everything is in-memory
- Instead of connection pools, you get direct access
- **Result**: a much shorter path for read-heavy, in-memory access patterns

## 🚀 The Core Philosophy: "We ARE the Database"

This became Lithair's foundation: What if, instead of connecting TO a database,
we simply ARE the database?

What if web development could be as simple as writing a single Rust file and
running `cargo build`?

**This isn't criticism—it's evolution.**

### From Idea to Production

**Traditional Path:**

```text
Brilliant Idea → Architecture Hell → 6 Months Later → Still Configuring → Idea Dies
```

**Lithair Path:**

```text
Brilliant Idea → Write Business Logic → cargo build → Ship to Production
```

## 🎯 Core Principles

### 1. **Simplicity Over Complexity**

_"The best architecture is the one you don't have to think about."_

Lithair tries to remove a large part of the architectural decision load by
providing opinionated defaults. The same binary can cover small deployments
and larger clustered setups, though the gains still depend on the workload and
operating model.

### 2. **Developer Happiness**

_"If you're not having fun building it, your users won't have fun using it."_

Web development should be joyful, not a chore. Lithair tries to bring back some
of that joy by letting you focus on what matters: your product.

### 3. **Performance by Design**

_"Why accept milliseconds when you can have nanoseconds?"_

By embedding the database in the application process, Lithair can remove one
common bottleneck for some workloads: network latency to a separate database.

### 4. **Natural Scalability**

_"Complexity should be optional, not mandatory."_

Start simple with a single binary. When you need to scale, the same codebase
can evolve toward clustered deployments without forcing a total rewrite, even
if production scaling still requires the usual operational work.

### 5. **Declarative Over Custom: The 90% Rule**

_"Most websites need the same things. Why keep rebuilding them?"_

Lithair is built around the idea that many websites share a recurring set of
core requirements:

- User authentication and sessions
- CRUD operations on data models
- Role-based permissions
- Static asset serving
- Form validation
- API endpoints

Instead of forcing you to implement these patterns from scratch every time,
Lithair provides **declarative defaults** that handle many common cases:

```rust
LithairServer::new()
    .with_rbac_config(rbac_config)           // Auth + sessions + roles
    .with_model_full::<Article>("/articles") // Full CRUD + RBAC
    .with_frontend("public")                 // Static serving
    .serve()                                 // That's it!
```

**The Philosophy:**

- ✅ **Declarative first**: Common patterns are built-in and configured, not coded
- ✅ **Less boilerplate**: No need to rewrite authentication handlers repeatedly
- ✅ **Convention over configuration**: Sensible defaults that work out of the box
- ✅ **Customizable when needed**: Every declarative feature can be overridden with custom logic

**But here's the key**: when you do need custom behavior, it stays explicit
and close to normal Rust code:

```rust
// Custom route when you need it
.with_route(Method::GET, "/special", |req| {
    Box::pin(async move {
        // Your custom logic here
        Ok(Response::new("Custom behavior"))
    })
})
```

**The balance:**

- 🎯 For 90% of use cases: Use declarative patterns, ship faster
- 🔧 For 10% of special needs: Drop into custom code, stay in control

This isn't about removing flexibility—it's about reducing the **need** for it
in common scenarios. Why keep rewriting authentication plumbing when
`.with_rbac_config()` can cover the standard case?

**The goal:** Spend your time on what makes YOUR app unique, not reimplementing authentication for the 100th time.

## 🌟 The Personal Journey

### Before Lithair

I was that developer who:

- Spent weeks setting up development environments
- Got lost in Docker configurations
- Fought with database migrations
- Deployed to 5 different services for a simple app
- Paid $20/month for infrastructure that served 10 users

### After Lithair

Now I:

- Write business logic from day one
- Deploy with `cargo build && ./my-app`
- Scale naturally when needed
- Often start with smaller infrastructure bills and scale later when needed
- Actually ship products instead of configuring them

## 🎯 The Mission

**Lithair exists to give developers their time back.**

Time to focus on:

- ✅ **Your users** instead of your infrastructure
- ✅ **Your product** instead of your deployment pipeline
- ✅ **Your ideas** instead of your database schema
- ✅ **Your creativity** instead of your configuration files

## 🌍 The Bigger Picture

## 💭 Why I Built This

People sometimes ask: "Why not just use existing solutions?"

Honestly, existing solutions work great. PostgreSQL, React, Express—they're all
solid tools that I use regularly.

But I kept finding myself in the same situation: wanting to build something
simple, then spending days setting up the infrastructure before writing any
actual business logic.

So I thought, "What if I just tried a different approach?" What if the database
was just... part of the app? No separate server, no connection strings, no
migrations.

Lithair is basically that experiment. It might be a terrible idea, but I wanted
to see what would happen.

Plus, honestly, it's also a learning project for me. I wanted to explore beyond
basic SQL and see what technologies like Raft consensus and event sourcing
could bring to modern web development. It's been pretty interesting to dive
into these concepts.

I should mention that this experimentation wouldn't have been possible without
AI assistance. The amount of techniques involved—Raft consensus, event
sourcing, HTTP parsing, Rust async programming—would have taken me years to
learn and implement on my own. Having an AI pair programming partner made it
possible to actually explore these ideas instead of just reading about them.

## 🤷 That's It

This might not make sense for most projects. If your current setup works well,
stick with it.

But if you've ever started a new project and felt overwhelmed by all the setup
before you could write your first line of business logic, maybe this approach
could be interesting.

It's just one way of doing things. Not better or worse, just different.

---

_Yoan Roblet_
_Creator of Lithair_
_"Making web development joyful, one binary at a time."_

---

**Want to be part of the story?**

- 🌟 **Star** the repository
- 🔧 **Contribute** to the codebase
- 📖 **Share** your Lithair success stories
- 💬 **Join** our community discussions
