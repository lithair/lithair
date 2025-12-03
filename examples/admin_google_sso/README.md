# Admin Dashboard with Google SSO

This example demonstrates how to protect an admin dashboard with Google OAuth2 authentication using Lithair's RBAC system.

## 🎯 Features

- **Google OAuth2 Login** - Users must authenticate with their Google account
- **Protected Admin Routes** - `/admin/*` routes require authentication
- **Automatic Redirection** - Unauthenticated users are redirected to Google login
- **Session Management** - OAuth tokens are managed securely
- **Role-Based Access** - Users are assigned roles after authentication

## 📋 Prerequisites

### 1. Google Cloud Console Setup

You need to create OAuth2 credentials in Google Cloud Console:

#### Step 1: Create a Project
1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project or select an existing one
3. Note your Project ID

#### Step 2: Enable Google+ API
1. Navigate to **APIs & Services** > **Library**
2. Search for "Google+ API"
3. Click **Enable**

#### Step 3: Configure OAuth Consent Screen
1. Go to **APIs & Services** > **OAuth consent screen**
2. Choose **External** (for testing) or **Internal** (for organization)
3. Fill in:
   - **App name**: Lithair Admin Demo
   - **User support email**: Your email
   - **Developer contact**: Your email
4. Click **Save and Continue**
5. **Scopes**: Add `openid`, `email`, and `profile`
6. Click **Save and Continue**
7. **Test users** (if External): Add your Google email
8. Click **Save and Continue**

#### Step 4: Create OAuth2 Credentials
1. Go to **APIs & Services** > **Credentials**
2. Click **Create Credentials** > **OAuth client ID**
3. Choose **Web application**
4. Fill in:
   - **Name**: Lithair Admin
   - **Authorized JavaScript origins**: `http://localhost:3000`
   - **Authorized redirect URIs**: `http://localhost:3000/auth/google/callback`
5. Click **Create**
6. **IMPORTANT**: Copy your **Client ID** and **Client Secret**

### 2. Environment Variables

Create a `.env` file in this directory:

```bash
# Google OAuth2 Credentials
GOOGLE_CLIENT_ID=your-client-id-here.apps.googleusercontent.com
GOOGLE_CLIENT_SECRET=your-client-secret-here
GOOGLE_REDIRECT_URI=http://localhost:3000/auth/google/callback

# Server Configuration
PORT=3000
```

**⚠️ SECURITY**: Never commit `.env` to git! It's already in `.gitignore`.

## 🚀 Running the Example

### Option 1: Using Taskfile (Recommended)

```bash
# From the project root
task examples:admin-google

# Or with custom port
task examples:admin-google PORT=8080
```

### Option 2: Manual

```bash
# Set environment variables
export GOOGLE_CLIENT_ID="your-client-id"
export GOOGLE_CLIENT_SECRET="your-secret"
export GOOGLE_REDIRECT_URI="http://localhost:3000/auth/google/callback"

# Run the example
cargo run -p admin_google_sso -- --port 3000
```

## 📖 Usage

### 1. Start the Server

```bash
task examples:admin-google
```

You should see:
```
🔐 Lithair Admin with Google SSO
==================================

🌐 Server listening on http://localhost:3000

📚 Endpoints:
   GET  /                        - Public homepage
   GET  /admin                   - Admin dashboard (requires Google login)
   GET  /auth/google/login       - Initiate Google OAuth2 flow
   GET  /auth/google/callback    - OAuth2 callback handler
   GET  /auth/logout             - Logout

💡 Try:
   1. Visit http://localhost:3000
   2. Click "Admin Dashboard"
   3. You'll be redirected to Google login
   4. After authentication, you'll see the admin panel
```

### 2. Access the Admin Dashboard

1. Open your browser to `http://localhost:3000`
2. Click **"Admin Dashboard"** or navigate to `/admin`
3. You'll be redirected to Google's login page
4. Sign in with your Google account
5. Grant permissions to the app
6. You'll be redirected back to `/admin` - now authenticated!

### 3. Test the Flow

```bash
# Public endpoint (no auth required)
curl http://localhost:3000/

# Admin endpoint (requires auth)
curl http://localhost:3000/admin
# Returns: Redirect to Google login

# After login, with session cookie
curl -b cookies.txt http://localhost:3000/admin
# Returns: Admin dashboard HTML
```

## 🔒 Security Features

### OAuth2 Flow
1. User clicks "Login with Google"
2. Server generates a random `state` parameter (CSRF protection)
3. User is redirected to Google's authorization page
4. User grants permissions
5. Google redirects back with an authorization `code`
6. Server exchanges `code` for an `access_token`
7. Server fetches user info from Google
8. Server creates a session and sets a secure cookie
9. User can now access protected routes

### CSRF Protection
- Random `state` parameter is generated and validated
- Prevents authorization code interception attacks

### Secure Cookies
- `HttpOnly` flag prevents JavaScript access
- `Secure` flag in production (HTTPS only)
- `SameSite=Lax` prevents CSRF

## 🎨 Customization

### Custom Role Mapping

Edit `src/main.rs` to map Google users to roles:

```rust
fn map_user_to_role(email: &str) -> String {
    if email.ends_with("@yourcompany.com") {
        "Admin".to_string()
    } else if email.ends_with("@partner.com") {
        "Manager".to_string()
    } else {
        "Viewer".to_string()
    }
}
```

### Custom Scopes

Request additional Google scopes:

```rust
let auth_url = format!(
    "https://accounts.google.com/o/oauth2/v2/auth?\
     scope=openid%20email%20profile%20https://www.googleapis.com/auth/calendar.readonly"
);
```

### Domain Restriction

Only allow specific domains:

```rust
if !user_info.email.ends_with("@yourcompany.com") {
    return Err(anyhow!("Only company emails allowed"));
}
```

## 🐛 Troubleshooting

### "redirect_uri_mismatch" Error
- Check that the redirect URI in Google Console exactly matches your `.env`
- Include the protocol (`http://` or `https://`)
- No trailing slash

### "access_denied" Error
- User declined permissions
- User not in test users list (if consent screen is in testing mode)

### "invalid_client" Error
- Check your `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET`
- Ensure credentials are for a "Web application" type

### Session Not Persisting
- Check that cookies are enabled in your browser
- Verify the cookie domain matches your server address

## 📚 Learn More

- [Google OAuth2 Documentation](https://developers.google.com/identity/protocols/oauth2)
- [Lithair RBAC Guide](../../docs/rbac.md)
- [OAuth2 RFC 6749](https://tools.ietf.org/html/rfc6749)

## 🎯 Next Steps

- Add **Microsoft** or **GitHub** OAuth providers
- Implement **JWT tokens** instead of sessions
- Add **role-based permissions** for different admin sections
- Enable **MFA** (Multi-Factor Authentication)
- Deploy to production with **HTTPS**
