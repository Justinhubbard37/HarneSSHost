# **OnTheLLow Brand Color Palette**

*Where tech meets technique. Turning complexity into clarity. We don't imitate, we innovate.*

## **Primary Brand Colors**

* **True Black (\`\#000000\`):** Page background  
* **Graphite (\`\#363543\`):** Card backgrounds and dark UI elements  
* **Cloudy Sky (\`\#5994d7\`):** Blue accent and section labels  
* **Azure Mist (\`\#e1f5f9\`):** Light text and backgrounds in light mode  
* **Mint Leaf (\`\#20bd90\`):** Primary accent, calls to action (CTAs), and Development category

## **Category Accents**

* **Amber (\`\#fa3205\`):** Security category accent  
* **Mint Leaf (\`\#20bd90\`):** Development category accent

## **Dark Theme UI Colors**

* **Foreground \- Azure Mist (\`\#e1f5f9\`):** Primary text  
* **Muted \- Spun Pearl (\`\#a1a1aa\`):** Secondary text  
* **Border \- Gun Powder (\`\#4a4958\`):** Card borders and dividers  
* **Card \- Steel Gray (\`\#1f1e2a\`):** Card and panel backgrounds

## **Recommended Format and Testing Hierarchy**

The most appropriate format and testing hierarchy for a startup brand color palette document should prioritize clarity, technical specification, and scalable application, especially for a digital-first brand like OnTheLLow.

## **Recommended Document Structure**

The document should be formatted to serve as a single source of truth for designers and developers, expanding on the current segmentation to be production-ready.

### **1\. Brand Overview (Foundation)**

**Purpose:** A brief statement defining the brand's identity and the emotion the colors should convey (e.g., "Where tech meets technique. Turning complexity into clarity.").  
**Color Mood/Rationale:** Briefly explain the conceptual meaning behind the core colors (e.g., Mint Leaf is for "Development" and represents growth).

### **2\. Color Hierarchy and Specification**

Organize colors by their role, including all necessary technical codes for digital and print application. This expands on the existing categories.

| Color Role | Example Color/Name | Required Specifications |
| ----- | ----- | ----- |
| **Primary Brand Colors** | True Black, Cloudy Sky, Mint Leaf | HEX Code (e.g., `#000000`), RGB/RGBA, HSL, CMYK (for print collateral), and SCSS/CSS Variable name (e.g., `$color-brand-primary`). |
| **Secondary/Accent Colors** | Amber (for Security), Mint Leaf (for CTAs) | Same technical specifications as Primary Colors, with a focus on their function (e.g., `$color-accent-security`). |
| **Neutral/Utility Colors** | Graphite, Azure Mist, Spun Pearl, Gun Powder | Same technical specifications, emphasizing their use for backgrounds, text, borders, and dividers. |
| **System/Status Colors** | (New Section) | Colors designated for system states: Success (Green, often Mint Leaf), Warning (Yellow/Orange), Error (Red/Amber), and Information (Blue). |

### **3\. Application Guidelines**

This section defines how colors are actually used, moving beyond simple role descriptions.  
**Color Pairing Matrix:** List accepted and forbidden color combinations, especially for text-on-background pairings (e.g., Azure Mist foreground on Graphite background in Dark Theme).  
**Theming/Variants:** Clearly define rules for application, such as the full palette swap for the Dark Theme UI Colors (Foreground, Muted, Border, Card). Include rules for transparency handling in color tokens.  
**Do's and Don'ts:** Provide examples of correct color application (e.g., only Mint Leaf for primary CTAs) and incorrect application (e.g., using a category accent color for primary body text).

## **Recommended Test Hierarchy (Validation)**

A systematic test hierarchy ensures the color palette maintains consistency, accessibility, and functional use across all applications.

| Test Level | Focus Area | Key Validation/Metric | Importance |
| ----- | ----- | ----- | ----- |
| **Level 1: Accessibility** | **Color Contrast & Readability** | Ensure all text/UI color pairings meet minimum [WCAG 2.1 AA/AAA contrast ratios](https://www.canva.com/learn/50-meticulous-style-guides-every-startup-see-launching/). This is critical for light text (Azure Mist) on dark backgrounds (True Black/Graphite). | **Critical** |
| **Level 2: Functionality** | **Interface State Consistency** | Validate that all color-coded interface elements (e.g., buttons, links, categories) maintain their color and usage in every state (Default, Hover, Active, Disabled). | **High** |
| **Level 3: Brand Consistency** | **Qualitative and Strategic Fit** | Review the palette's application against the overall brand strategy, ensuring the colors align with the intended tone ("Where tech meets technique") and visual identity across mockups. | **Moderate** |

