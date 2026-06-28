# -*- coding: utf-8 -*-
"""
get_refresh_token.py

Automates retrieving a Google Drive OAuth2 refresh token.
"""

import http.server
import socketserver
import webbrowser
import urllib.parse
import urllib.request
import json
import re
import sys
from pathlib import Path

PORT = 8080
REDIRECT_URI = f"http://localhost:{PORT}"
CONFIG_PATH = Path(sys.argv[1] if len(sys.argv) > 1 else "config.toml")

def read_config():
    """
    Reads the configuration file and extracts the Google Client ID and client secret.

    Returns:
        tuple: A tuple containing (client_id, client_secret).
    """
    if not CONFIG_PATH.exists():
        print(f"Error: {CONFIG_PATH} not found.")
        sys.exit(1)
    
    content = CONFIG_PATH.read_text()
    
    # Simple regex parsing to avoid external dependencies
    client_id_match = re.search(r'client_id\s*=\s*"([^"]+)"', content)
    client_secret_match = re.search(r'client_secret\s*=\s*"([^"]+)"', content)
    
    if not client_id_match or not client_secret_match:
        print("Error: Could not find client_id or client_secret in config.toml.")
        print("Please configure them first under [google_credentials].")
        sys.exit(1)
        
    return client_id_match.group(1), client_secret_match.group(1)

def update_config(refresh_token):
    """
    Updates the configuration file with the new Google refresh token.

    Args:
        refresh_token (str): The newly retrieved refresh token.
    """
    content = CONFIG_PATH.read_text()
    # Replace refresh_token value
    new_content = re.sub(
        r'(refresh_token\s*=\s*")[^"]*(")',
        rf'\g<1>{refresh_token}\g<2>',
        content
    )
    CONFIG_PATH.write_text(new_content)
    print(f"\n[+] Successfully updated {CONFIG_PATH} with the new refresh_token!")

def exchange_code_for_token(code, client_id, client_secret):
    """
    Exchanges the authorization code for a Google refresh token.

    Args:
        code (str): The authorization code.
        client_id (str): Google API client ID.
        client_secret (str): Google API client secret.

    Returns:
        str: The retrieved refresh token, or None if failed.
    """
    url = "https://oauth2.googleapis.com/token"
    data = urllib.parse.urlencode({
        "code": code,
        "client_id": client_id,
        "client_secret": client_secret,
        "redirect_uri": REDIRECT_URI,
        "grant_type": "authorization_code"
    }).encode("utf-8")
    
    req = urllib.request.Request(url, data=data)
    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            return res_data.get("refresh_token")
    except Exception as e:
        print(f"Error exchanging code for token: {e}")
        if hasattr(e, 'read'):
            print(e.read().decode("utf-8"))
        sys.exit(1)

class OAuthHandler(http.server.SimpleHTTPRequestHandler):
    """
    HTTP Request Handler to receive the OAuth2 authorization code callback.
    """
    auth_code = None

    def do_GET(self):
        """
        Handles the HTTP GET request containing the OAuth2 callback query params.
        """
        query = urllib.parse.urlparse(self.path).query
        params = urllib.parse.parse_qs(query)
        
        if "code" in params:
            OAuthHandler.auth_code = params["code"][0]
            self.send_response(200)
            self.send_header("Content-type", "text/html")
            self.end_headers()
            self.wfile.write(b"<html><body><h1>Authorization Successful!</h1><p>You can close this tab and return to the terminal.</p></body></html>")
        else:
            self.send_response(400)
            self.send_header("Content-type", "text/html")
            self.end_headers()
            self.wfile.write(b"<html><body><h1>Failed to authorize</h1></body></html>")

    def log_message(self, format, *args):
        """
        Suppresses log messages for local redirect requests.
        """
        pass # Suppress server logs

def run_server():
    """
    Runs a temporary local socket server to wait for the OAuth2 redirect callback.

    Returns:
        str: The retrieved authorization code.
    """
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("", PORT), OAuthHandler) as httpd:
        print(f"[*] Temporary server listening on {REDIRECT_URI}...")
        while OAuthHandler.auth_code is None:
            httpd.handle_request()
    return OAuthHandler.auth_code

def main():
    """
    Executes the Google OAuth2 refresh token retrieval flow.
    """
    client_id, client_secret = read_config()
    
    # Build OAuth URL
    auth_url = (
        "https://accounts.google.com/o/oauth2/v2/auth?"
        "scope=https://www.googleapis.com/auth/drive&"
        "access_type=offline&"
        "prompt=consent&"
        "response_type=code&"
        f"redirect_uri={urllib.parse.quote(REDIRECT_URI)}&"
        f"client_id={client_id}"
    )
    
    print("\n" + "="*80)
    print("1. Copy and paste this URL into your browser to log in:")
    print("="*80)
    print(auth_url)
    print("="*80 + "\n")
    
    # Try to open the browser automatically
    try:
        webbrowser.open(auth_url)
    except Exception:
        pass
    
    print("2. After authorizing, your browser will redirect to a page that looks like it failed")
    print("   (e.g., http://localhost:8080/?code=4/0Ad...).")
    print("   Copy the entire redirect URL from the browser address bar.\n")
    
    redirected_url = input("3. Paste the copied redirect URL (or authorization code) here: ").strip()
    
    # Extract code from URL if they pasted the whole URL
    code = redirected_url
    if "code=" in redirected_url:
        parsed = urllib.parse.urlparse(redirected_url)
        params = urllib.parse.parse_qs(parsed.query)
        if "code" in params:
            code = params["code"][0]
            
    if not code:
        print("[-] Error: No code entered.")
        sys.exit(1)
        
    print("\n[*] Exchanging code for refresh token...")
    refresh_token = exchange_code_for_token(code, client_id, client_secret)
    
    if refresh_token:
        print(f"[+] Refresh Token: {refresh_token}")
        update_config(refresh_token)
    else:
        print("[-] Error: Did not receive a refresh token. Check your credentials and try again.")

if __name__ == "__main__":
    main()
